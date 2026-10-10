# Windows isolation wrapper for verify-mediaflick: runs one command on a new,
# inactive desktop inside a kill-on-close job. Never calls SwitchDesktop, so
# nothing appears on the user's screen and nothing takes focus.
#
#   pwsh -NoProfile -File isolate-windows.ps1 node session.mjs <drive> ...
#
# Every argument is the command, verbatim (no parameter binding, so the
# command's own --flags pass through). MEDIAFLICK_VERIFY_HARD_TIMEOUT
# (seconds, default 900) bounds the whole run; the job is terminated after it.
#
# The child (and everything it starts, such as the app) inherits the private
# desktop. MEDIAFLICK_VERIFY_ISOLATION tells the child which desktop it was
# given so it can confirm it really runs there before launching anything.
$ErrorActionPreference = 'Stop'
if ($env:MEDIAFLICK_VERIFY_ISOLATION) { throw 'Already inside a verify-mediaflick isolation wrapper; refusing to nest.' }
$TimeoutSeconds = if ($env:MEDIAFLICK_VERIFY_HARD_TIMEOUT) { [int]$env:MEDIAFLICK_VERIFY_HARD_TIMEOUT } else { 900 }
if ($TimeoutSeconds -lt 1 -or $TimeoutSeconds -gt 3600) { throw 'MEDIAFLICK_VERIFY_HARD_TIMEOUT must be 1..3600.' }
$Command = @($args)
if ($Command.Count -eq 0) { throw 'No command given.' }

$exe = (Get-Command $Command[0] -CommandType Application | Select-Object -First 1).Source
if (!$exe) { throw "Cannot find $($Command[0])." }
if ($exe -match '\\WindowsApps\\') {
  # Children of an MSIX-packaged host start outside its job; refuse rather than leak.
  throw "$exe is MSIX-packaged; install the regular (MSI) build instead."
}

if (!('MediaFlickVerifyDesktop' -as [type])) {
  Add-Type @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;

public static class MediaFlickVerifyDesktop {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct STARTUPINFO {
        public int cb; public string lpReserved; public string lpDesktop; public string lpTitle;
        public int dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars, dwFillAttribute, dwFlags;
        public short wShowWindow, cbReserved2; public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct PROCESS_INFORMATION { public IntPtr hProcess, hThread; public int dwProcessId, dwThreadId; }
    [StructLayout(LayoutKind.Sequential)]
    struct BASIC_LIMIT { public long PerProcessUserTimeLimit, PerJobUserTimeLimit; public uint LimitFlags; public UIntPtr MinimumWorkingSetSize, MaximumWorkingSetSize; public uint ActiveProcessLimit; public UIntPtr Affinity; public uint PriorityClass, SchedulingClass; }
    [StructLayout(LayoutKind.Sequential)]
    struct IO_COUNTERS { public ulong a, b, c, d, e, f; }
    [StructLayout(LayoutKind.Sequential)]
    struct EXTENDED_LIMIT { public BASIC_LIMIT Basic; public IO_COUNTERS Io; public UIntPtr ProcessMemoryLimit, JobMemoryLimit, PeakProcessMemoryUsed, PeakJobMemoryUsed; }
    [StructLayout(LayoutKind.Sequential)]
    struct ACCOUNTING { public long TotalUserTime, TotalKernelTime, ThisPeriodTotalUserTime, ThisPeriodTotalKernelTime; public uint TotalPageFaultCount, TotalProcesses, ActiveProcesses, TotalTerminatedProcesses; }

    [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Unicode)] static extern IntPtr CreateDesktopW(string name, IntPtr device, IntPtr mode, uint flags, uint access, IntPtr sa);
    [DllImport("user32.dll")] static extern bool CloseDesktop(IntPtr desktop);
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] static extern IntPtr CreateJobObjectW(IntPtr sa, string name);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetInformationJobObject(IntPtr job, int cls, ref EXTENDED_LIMIT info, int size);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool QueryInformationJobObject(IntPtr job, int cls, out ACCOUNTING info, int size, IntPtr ret);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool TerminateJobObject(IntPtr job, uint code);
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] static extern bool CreateProcessW(string app, StringBuilder cmd, IntPtr pa, IntPtr ta, bool inherit, uint flags, IntPtr env, string cwd, ref STARTUPINFO si, out PROCESS_INFORMATION pi);
    [DllImport("kernel32.dll", SetLastError = true)] static extern uint ResumeThread(IntPtr thread);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool TerminateProcess(IntPtr process, uint code);
    [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr handle, uint ms);
    [DllImport("kernel32.dll")] static extern bool GetExitCodeProcess(IntPtr process, out uint code);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll")] static extern IntPtr GetStdHandle(int kind);
    [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool DuplicateHandle(IntPtr sp, IntPtr s, IntPtr tp, out IntPtr t, uint access, bool inherit, uint options);

    static void Check(bool ok, string message) {
        if (!ok) throw new Win32Exception(Marshal.GetLastWin32Error(), message);
    }
    static IntPtr Inheritable(int kind) {
        var original = GetStdHandle(kind);
        if (original == IntPtr.Zero || original == new IntPtr(-1)) return IntPtr.Zero;
        IntPtr copy;
        Check(DuplicateHandle(GetCurrentProcess(), original, GetCurrentProcess(), out copy, 0, true, 2), "Cannot pass console handles");
        return copy;
    }

    // Returns the child's exit code; 124 when the timeout expired.
    public static int Run(string desktopName, string app, string commandLine, int timeoutSeconds) {
        // DESKTOP_CREATEWINDOW | READOBJECTS | WRITEOBJECTS | ENUMERATE | SWITCHDESKTOP is not requested on purpose.
        var desktop = CreateDesktopW(desktopName, IntPtr.Zero, IntPtr.Zero, 0, 0x0002 | 0x0001 | 0x0080 | 0x0040, IntPtr.Zero);
        Check(desktop != IntPtr.Zero, "Cannot create a private desktop; refusing to run on the user's desktop");
        var job = IntPtr.Zero;
        IntPtr stdout = IntPtr.Zero, stderr = IntPtr.Zero;
        var pi = new PROCESS_INFORMATION();
        try {
            job = CreateJobObjectW(IntPtr.Zero, "Local\\" + desktopName);
            Check(job != IntPtr.Zero, "Cannot create the verification job");
            var limits = new EXTENDED_LIMIT();
            limits.Basic.LimitFlags = 0x2000; // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            Check(SetInformationJobObject(job, 9, ref limits, Marshal.SizeOf(typeof(EXTENDED_LIMIT))), "Cannot configure the verification job");

            var si = new STARTUPINFO();
            si.cb = Marshal.SizeOf(typeof(STARTUPINFO));
            si.lpDesktop = "WinSta0\\" + desktopName;
            si.dwFlags = 0x100; // STARTF_USESTDHANDLES
            stdout = Inheritable(-11); stderr = Inheritable(-12);
            si.hStdOutput = stdout; si.hStdError = stderr;
            // CREATE_SUSPENDED | CREATE_NO_WINDOW: contained before it runs, no console on any desktop.
            Check(CreateProcessW(app, new StringBuilder(commandLine), IntPtr.Zero, IntPtr.Zero, true, 0x4 | 0x08000000, IntPtr.Zero, null, ref si, out pi), "Cannot start the isolated command");
            if (!AssignProcessToJobObject(job, pi.hProcess)) {
                var code = Marshal.GetLastWin32Error();
                TerminateProcess(pi.hProcess, 1);
                throw new Win32Exception(code, "Cannot contain the isolated command; it was not started");
            }
            Check(ResumeThread(pi.hThread) != uint.MaxValue, "Cannot start the isolated command");
            uint exit;
            if (WaitForSingleObject(pi.hProcess, (uint)timeoutSeconds * 1000) == 0x102) {
                Console.Error.WriteLine("isolate-windows: timed out after " + timeoutSeconds + " s; terminating the job.");
                exit = 124;
            } else {
                GetExitCodeProcess(pi.hProcess, out exit);
            }
            TerminateJobObject(job, exit);
            for (var i = 0; i < 250; i++) {
                ACCOUNTING info;
                if (!QueryInformationJobObject(job, 1, out info, Marshal.SizeOf(typeof(ACCOUNTING)), IntPtr.Zero) || info.ActiveProcesses == 0) break;
                System.Threading.Thread.Sleep(20);
            }
            return (int)exit;
        } finally {
            if (pi.hThread != IntPtr.Zero) CloseHandle(pi.hThread);
            if (pi.hProcess != IntPtr.Zero) CloseHandle(pi.hProcess);
            if (stdout != IntPtr.Zero) CloseHandle(stdout);
            if (stderr != IntPtr.Zero) CloseHandle(stderr);
            if (job != IntPtr.Zero) CloseHandle(job);
            CloseDesktop(desktop);
        }
    }
}
'@
}

function Quote([string] $arg) {
  if ($arg -and $arg -notmatch '[\s"]') { return $arg }
  # CommandLineToArgvW rules: double backslashes before a quote, escape quotes.
  '"' + ($arg -replace '(\\*)"', '$1$1\"' -replace '(\\+)$', '$1$1') + '"'
}

$name = "MediaFlick.Verify.$PID.$([Environment]::TickCount64)"
$env:MEDIAFLICK_VERIFY_ISOLATION = "windows-desktop:$name"
$commandLine = (@($exe) + @($Command | Select-Object -Skip 1) | ForEach-Object { Quote $_ }) -join ' '
$code = [MediaFlickVerifyDesktop]::Run($name, $exe, $commandLine, $TimeoutSeconds)
exit $code

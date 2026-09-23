use std::ffi::c_void;

use windows::Win32::Foundation::{HANDLE, HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11CreateDevice,
    ID3D11Device, ID3D11Device1, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_TYPELESS,
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_R8G8B8A8_TYPELESS,
    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter, IDXGIDevice,
    IDXGIFactory1, IDXGIFactory2, IDXGISwapChain1,
};
use windows::core::Interface;

pub(super) struct Compositor {
    composition: IDCompositionDevice,
    _target: IDCompositionTarget,
    content: Layer,
    popup: Layer,
    gpu: Option<GpuDevice>,
}

impl Compositor {
    pub(super) fn new(raw_window: usize) -> windows::core::Result<Self> {
        let window = HWND(raw_window as *mut c_void);
        let composition: IDCompositionDevice =
            unsafe { DCompositionCreateDevice(None::<&IDXGIDevice>)? };
        let target = unsafe { composition.CreateTargetForHwnd(window, false)? };
        let root = unsafe { composition.CreateVisual()? };
        let content_visual = unsafe { composition.CreateVisual()? };
        let popup_visual = unsafe { composition.CreateVisual()? };
        unsafe {
            content_visual.AddVisual(&popup_visual, true, None::<&IDCompositionVisual>)?;
            root.AddVisual(&content_visual, true, None::<&IDCompositionVisual>)?;
            target.SetRoot(&root)?;
            composition.Commit()?;
        }
        Ok(Self {
            composition,
            _target: target,
            content: Layer::new(content_visual, true),
            popup: Layer::new(popup_visual, false),
            gpu: None,
        })
    }

    pub(super) fn present_shared(
        &mut self,
        popup: bool,
        shared_handle: *mut c_void,
    ) -> windows::core::Result<()> {
        if shared_handle.is_null() {
            return Ok(());
        }
        // CEF owns the handle. We open a D3D reference during this callback
        // and deliberately never close the borrowed handle itself.
        if self.gpu.is_none() {
            self.gpu = Some(GpuDevice::open_for_shared_texture(shared_handle)?);
        }
        let Some(gpu) = self.gpu.as_ref() else {
            return Err(unexpected_error(
                "D3D11 device initialization returned no device",
            ));
        };
        let source = gpu.open_shared_texture(shared_handle)?;
        let mut source_desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { source.GetDesc(&mut source_desc) };
        let layer = if popup {
            &mut self.popup
        } else {
            &mut self.content
        };
        let rebound = layer.present_shared(gpu, &source, source_desc)?;
        if rebound {
            unsafe { self.composition.Commit()? };
        }
        Ok(())
    }

    pub(super) fn present_software(
        &mut self,
        popup: bool,
        pixels: &[u8],
        width: i32,
        height: i32,
    ) -> windows::core::Result<()> {
        if pixels.is_empty() || width <= 0 || height <= 0 {
            return Ok(());
        }
        if self.gpu.is_none() {
            self.gpu = Some(GpuDevice::default_hardware()?);
        }
        let Some(gpu) = self.gpu.as_ref() else {
            return Err(unexpected_error(
                "D3D11 device initialization returned no device",
            ));
        };
        let layer = if popup {
            &mut self.popup
        } else {
            &mut self.content
        };
        let rebound = layer.present_software(gpu, pixels, width as u32, height as u32)?;
        if rebound {
            unsafe { self.composition.Commit()? };
        }
        Ok(())
    }

    pub(super) fn set_popup_visible(&mut self, visible: bool) {
        if self.popup.set_visible(visible) {
            self.commit();
        }
    }

    pub(super) fn set_popup_position(&mut self, x: f32, y: f32) {
        unsafe {
            let _ = self.popup.visual.SetOffsetX2(x);
            let _ = self.popup.visual.SetOffsetY2(y);
        }
        self.commit();
    }

    fn commit(&self) {
        if let Err(error) = unsafe { self.composition.Commit() } {
            tracing::warn!(target: "cef.osr", "DirectComposition commit failed: {error}");
        }
    }
}

struct GpuDevice {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    factory: IDXGIFactory2,
}

impl GpuDevice {
    fn default_hardware() -> windows::core::Result<Self> {
        let factory = unsafe { CreateDXGIFactory1::<IDXGIFactory1>()? };
        let adapter = unsafe { factory.EnumAdapters1(0)? };
        Self::on_adapter(&adapter.cast()?)
    }

    fn open_for_shared_texture(shared_handle: *mut c_void) -> windows::core::Result<Self> {
        let factory = unsafe { CreateDXGIFactory1::<IDXGIFactory1>()? };
        let mut index = 0;
        loop {
            let Ok(adapter) = (unsafe { factory.EnumAdapters1(index) }) else {
                break;
            };
            index += 1;
            let Ok(gpu) = Self::on_adapter(&adapter.cast()?) else {
                continue;
            };
            if gpu.open_shared_texture(shared_handle).is_ok() {
                return Ok(gpu);
            }
        }
        Err(unexpected_error(
            "no D3D11 adapter could open CEF's shared texture",
        ))
    }

    fn on_adapter(adapter: &IDXGIAdapter) -> windows::core::Result<Self> {
        let mut device = None;
        let mut context = None;
        let mut feature_level = D3D_FEATURE_LEVEL::default();
        unsafe {
            D3D11CreateDevice(
                adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                Some(&mut feature_level),
                Some(&mut context),
            )?;
        }
        let device = device.ok_or_else(|| unexpected_error("D3D11 returned no device"))?;
        let context = context.ok_or_else(|| unexpected_error("D3D11 returned no context"))?;
        let dxgi_device: IDXGIDevice = device.cast()?;
        let adapter = unsafe { dxgi_device.GetAdapter()? };
        let factory = unsafe { adapter.GetParent::<IDXGIFactory2>()? };
        Ok(Self {
            device,
            context,
            factory,
        })
    }

    fn open_shared_texture(
        &self,
        shared_handle: *mut c_void,
    ) -> windows::core::Result<ID3D11Texture2D> {
        let mut texture = None;
        let legacy_result = unsafe {
            self.device
                .OpenSharedResource(HANDLE(shared_handle), &mut texture)
        };
        if legacy_result.is_ok()
            && let Some(texture) = texture
        {
            return Ok(texture);
        }
        let device: ID3D11Device1 = self.device.cast()?;
        unsafe { device.OpenSharedResource1(HANDLE(shared_handle)) }
    }
}

struct Layer {
    visual: IDCompositionVisual,
    swap_chain: Option<IDXGISwapChain1>,
    swap_chain_key: Option<SwapChainKey>,
    visible: bool,
    attached: bool,
}

impl Layer {
    fn new(visual: IDCompositionVisual, visible: bool) -> Self {
        Self {
            visual,
            swap_chain: None,
            swap_chain_key: None,
            visible,
            attached: false,
        }
    }

    fn present_shared(
        &mut self,
        gpu: &GpuDevice,
        source: &ID3D11Texture2D,
        source_desc: D3D11_TEXTURE2D_DESC,
    ) -> windows::core::Result<bool> {
        if !self.visible {
            return Ok(false);
        }
        let rebound = self.ensure_swap_chain(
            gpu,
            SwapChainKey {
                width: source_desc.Width,
                height: source_desc.Height,
                format: swap_chain_format(source_desc.Format),
            },
        )?;
        let Some(swap_chain) = self.swap_chain.as_ref() else {
            return Err(unexpected_error("DXGI returned no content swap chain"));
        };
        let back_buffer = unsafe { swap_chain.GetBuffer::<ID3D11Texture2D>(0)? };
        unsafe {
            gpu.context.CopyResource(&back_buffer, source);
            swap_chain.Present(1, DXGI_PRESENT(0)).ok()?;
        }
        Ok(rebound)
    }

    fn present_software(
        &mut self,
        gpu: &GpuDevice,
        pixels: &[u8],
        width: u32,
        height: u32,
    ) -> windows::core::Result<bool> {
        if !self.visible {
            return Ok(false);
        }
        let rebound = self.ensure_swap_chain(
            gpu,
            SwapChainKey {
                width,
                height,
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
            },
        )?;
        let Some(swap_chain) = self.swap_chain.as_ref() else {
            return Err(unexpected_error("DXGI returned no software swap chain"));
        };
        let back_buffer = unsafe { swap_chain.GetBuffer::<ID3D11Texture2D>(0)? };
        // `pixels` was length-checked against width * height * 4 by the CEF
        // paint bridge, so UpdateSubresource can read every advertised row.
        unsafe {
            gpu.context.UpdateSubresource(
                &back_buffer,
                0,
                None,
                pixels.as_ptr().cast(),
                width.saturating_mul(4),
                0,
            );
            swap_chain.Present(1, DXGI_PRESENT(0)).ok()?;
        }
        Ok(rebound)
    }

    fn ensure_swap_chain(
        &mut self,
        gpu: &GpuDevice,
        key: SwapChainKey,
    ) -> windows::core::Result<bool> {
        if self.swap_chain_key != Some(key) {
            let description = DXGI_SWAP_CHAIN_DESC1 {
                Width: key.width,
                Height: key.height,
                Format: key.format,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                ..DXGI_SWAP_CHAIN_DESC1::default()
            };
            let swap_chain = unsafe {
                gpu.factory.CreateSwapChainForComposition(
                    &gpu.device,
                    &description,
                    None::<&windows::Win32::Graphics::Dxgi::IDXGIOutput>,
                )?
            };
            unsafe { self.visual.SetContent(&swap_chain)? };
            self.swap_chain = Some(swap_chain);
            self.swap_chain_key = Some(key);
            self.attached = true;
            return Ok(true);
        }
        if !self.attached
            && let Some(swap_chain) = self.swap_chain.as_ref()
        {
            unsafe { self.visual.SetContent(swap_chain)? };
            self.attached = true;
            return Ok(true);
        }
        Ok(false)
    }

    fn set_visible(&mut self, visible: bool) -> bool {
        if self.visible == visible {
            return false;
        }
        self.visible = visible;
        if !visible && self.attached {
            let _ = unsafe { self.visual.SetContent(None::<&windows::core::IUnknown>) };
            self.attached = false;
        }
        true
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct SwapChainKey {
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
}

fn swap_chain_format(format: DXGI_FORMAT) -> DXGI_FORMAT {
    match format {
        DXGI_FORMAT_B8G8R8A8_TYPELESS | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB => {
            DXGI_FORMAT_B8G8R8A8_UNORM
        }
        DXGI_FORMAT_R8G8B8A8_TYPELESS | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB => {
            DXGI_FORMAT_R8G8B8A8_UNORM
        }
        other => other,
    }
}

fn unexpected_error(message: &str) -> windows::core::Error {
    windows::core::Error::new(windows::core::HRESULT(0x8000_4005_u32 as i32), message)
}

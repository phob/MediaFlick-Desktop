### Fixed

- Quick Connect now stops polling when a code expires or fails, shows a clear expiration message, and lets you request a new code without editing the server address.
- Cancelling an in-flight Quick Connect poll no longer signs you in after you leave or change the server address.
- If checking user permissions fails at startup, MediaFlick now retries automatically once the server responds so collections and missing titles do not stay hidden for the rest of the session.
- A slow sign-out request can no longer sign out a newer session started while it was still running.
- Home and appearance settings saves now stay bound to the signed-in account and cannot write to a different account during an account switch.

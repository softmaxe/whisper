//! Network services: self-hosted transcription, text cleanup, model listing
//! and file uploads. Everything async runs on the shared runtime in
//! [`runtime`]; spawn service futures with [`runtime::spawn`] and await the
//! handle from any executor.

mod asr;
pub mod cleanup;
pub mod ffmpeg;
mod http_body;
pub mod retry;
pub mod runtime;
pub mod transcription;
pub mod upload;

#[cfg(test)]
pub(crate) mod live_tests;
#[cfg(test)]
mod performance_tests;
#[cfg(test)]
pub(crate) mod test_server;

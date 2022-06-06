use napi_derive::*;

#[napi]
pub fn initTracing() {
    tracing_subscriber::fmt::init();
}

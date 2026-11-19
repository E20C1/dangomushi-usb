pub mod contract;

#[cfg(target_os = "android")]
mod android_bridge;
#[cfg(target_os = "android")]
mod app;

#[cfg(target_os = "android")]
#[no_mangle]
pub fn android_main(android_app: winit::platform::android::activity::AndroidApp) {
    app::run(android_app);
}

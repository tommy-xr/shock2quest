pub mod assets;
pub mod audio;
mod builtin_font;
pub mod dds;
mod engine;
pub mod file_system;
mod font;
mod gl_engine;
pub mod importers;
pub mod logging;
pub mod macros;
pub mod materials;
pub mod platform;
pub mod scene;
mod shader;
mod shader_program;
pub mod texture;
pub mod texture_atlas;
pub mod texture_format;
pub mod util;

pub use crate::builtin_font::{BuiltinFont, shared_builtin_font};
pub use crate::engine::Engine;
pub use crate::engine::EngineRenderContext;
pub use crate::font::{Font, FontCharacterInfo, ellipsize, measure_text_width};

pub fn opengl() -> Box<dyn Engine> {
    let engine = gl_engine::init_gl();
    Box::new(engine)
}

pub fn opengles() -> Box<dyn Engine> {
    let engine = gl_engine::init_gles();
    Box::new(engine)
}

#[cfg(target_os = "android")]
pub fn android() -> Box<dyn Engine> {
    let engine = gl_engine::init_android();
    Box::new(engine)
}

#[cfg(test)]
mod platform_event_pump_tests {
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    static CALLS: AtomicUsize = AtomicUsize::new(0);
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn count_call() {
        CALLS.fetch_add(1, Ordering::Relaxed);
    }

    #[test]
    fn services_the_callback_registered_for_the_current_thread() {
        let _guard = TEST_LOCK.lock().unwrap();
        CALLS.store(0, Ordering::Relaxed);
        crate::platform::set_event_pump(Some(count_call));

        crate::platform::service_events();

        assert_eq!(CALLS.load(Ordering::Relaxed), 1);
        crate::platform::set_event_pump(None);
    }

    #[test]
    fn does_not_run_a_main_thread_callback_on_a_worker() {
        let _guard = TEST_LOCK.lock().unwrap();
        CALLS.store(0, Ordering::Relaxed);
        crate::platform::set_event_pump(Some(count_call));

        std::thread::spawn(crate::platform::service_events)
            .join()
            .unwrap();

        assert_eq!(CALLS.load(Ordering::Relaxed), 0);
        crate::platform::service_events();
        assert_eq!(CALLS.load(Ordering::Relaxed), 1);
        crate::platform::set_event_pump(None);
    }
}

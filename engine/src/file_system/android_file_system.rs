pub use crate::file_system::FileSystem;
use std::ffi::CString;

pub struct AndroidFileSystem {
    ctx: ndk_context::AndroidContext,
    asset_manager: ndk::asset::AssetManager,
}

impl FileSystem for AndroidFileSystem {
    fn open_dir(&self, path: &str) -> Vec<String> {
        let vm = unsafe { jni::JavaVM::from_raw(self.ctx.vm().cast()) }.unwrap();
        let env = vm.attach_current_thread().unwrap();
        let asset_manager = env
            .call_method(
                self.ctx.context().cast(),
                //resource_manager.cast(),
                "getAssets",
                "()Landroid/content/res/AssetManager;",
                &[],
            )
            .unwrap()
            .l()
            .unwrap();

        let dirs = env
            .call_method(
                asset_manager,
                "list",
                "(Ljava/lang/String;)[Ljava/lang/String;",
                &[env.new_string(path).unwrap().into()],
            )
            .unwrap()
            .l()
            .unwrap()
            .into_inner();
        let mut out = vec![];
        let len = env.get_array_length(dirs).unwrap();
        for i in 0..len {
            let folder = env.get_object_array_element(dirs, i).unwrap();
            let folder_string: String = env.get_string(folder.into()).unwrap().into();
            out.push(folder_string);
        }
        out
    }

    fn open_file(&self, path: &str) -> Vec<u8> {
        let mut asset = self
            .asset_manager
            .open(&CString::new(path).unwrap())
            .expect("Could not open path");
        let data = asset.get_buffer().unwrap();
        data.to_vec()
    }
}

use ndk::asset::AssetManager;

pub fn init() -> AndroidFileSystem {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.unwrap();
    let env = vm.attach_current_thread().unwrap();

    // Query the global Audio Service
    let _class_ctxt = env.find_class("android/content/Context").unwrap();
    let jni_asset_manager = env
        .call_method(
            ctx.context().cast(),
            //resource_manager.cast(),
            "getAssets",
            "()Landroid/content/res/AssetManager;",
            &[],
        )
        .unwrap()
        .l()
        .unwrap();

    let asset_manager_ptr = unsafe {
        ndk_sys::AAssetManager_fromJava(env.get_native_interface(), jni_asset_manager.cast())
    };

    let ptr2 = std::ptr::NonNull::new(asset_manager_ptr).unwrap();
    let asset_manager = unsafe { AssetManager::from_ptr(ptr2) };

    AndroidFileSystem { ctx, asset_manager }
}

use jni::signature::{JavaType, Primitive};

// TODO: Ported from this project:
// https://github.com/dodorare/crossbow/blob/122387f6ec84f8591b400686cccde0ff390f9f61/platform/android/src/permission/request_permission.rs

/// Get `PERMISSION_GRANTED` and `PERMISSION_DENIED` statuses.
fn permission_status(jnienv: &jni::JNIEnv) -> Result<(i32, i32), jni::errors::Error> {
    let class_package_manager = jnienv.find_class("android/content/pm/PackageManager")?;
    let field_permission_granted =
        jnienv.get_static_field_id(class_package_manager, "PERMISSION_GRANTED", "I")?;

    let field_permission_denied =
        jnienv.get_static_field_id(class_package_manager, "PERMISSION_DENIED", "I")?;

    let permission_denied = jnienv.get_static_field_unchecked(
        class_package_manager,
        field_permission_denied,
        JavaType::Primitive(Primitive::Int),
    )?;

    let permission_granted = jnienv.get_static_field_unchecked(
        class_package_manager,
        field_permission_granted,
        JavaType::Primitive(Primitive::Int),
    )?;

    Ok((permission_granted.i()?, permission_denied.i()?))
}

/// Get `PERMISSION_GRANTED` and `PERMISSION_DENIED` statuses.
/// Provides checking permission status in the application and will request permission if
/// it is denied.
fn request_permission_inner() -> Result<bool, jni::errors::Error> {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.unwrap();
    let jnienv = vm.attach_current_thread_as_daemon().unwrap();

    let READ_PERMISSION = jnienv
        .new_string("android.permission.READ_EXTERNAL_STORAGE")
        .unwrap();
    let WRITE_PERMISSION = jnienv
        .new_string("android.permission.WRITE_EXTERNAL_STORAGE")
        .unwrap();

    let (permission_granted, permission_denied) = permission_status(&jnienv).unwrap();

    // Determine whether you have been granted a particular permission.
    let class_context = jnienv.find_class("android/content/Context")?;
    let method_check_self_permission = jnienv.get_method_id(
        class_context,
        "checkSelfPermission",
        "(Ljava/lang/String;)I",
    )?;

    let ret = jnienv.call_method_unchecked(
        ndk_context::android_context().context().cast(),
        method_check_self_permission,
        JavaType::Primitive(Primitive::Int),
        &[READ_PERMISSION.into()],
    )?;

    tracing::debug!(
        "Permission check result: {:?} (granted={:?}, denied={:?})",
        ret,
        permission_granted,
        permission_denied
    );

    //return Ok(true);
    if ret.i()? == permission_granted {
        return Ok(true);
    }

    let array_permissions = jnienv.new_object_array(
        2,
        jnienv.find_class("java/lang/String")?,
        jnienv.new_string(String::new())?,
    )?;

    jnienv.set_object_array_element(array_permissions, 0, READ_PERMISSION);
    jnienv.set_object_array_element(array_permissions, 1, WRITE_PERMISSION);
    let class_activity = jnienv.find_class("android/app/Activity")?;
    tracing::info!("Requesting Android permissions...");
    let method_request_permissions = jnienv.get_method_id(
        class_activity,
        "requestPermissions",
        "([Ljava/lang/String;I)V",
    )?;

    jnienv.call_method_unchecked(
        ndk_context::android_context().context().cast(),
        method_request_permissions,
        JavaType::Primitive(Primitive::Void),
        &[array_permissions.into(), jni::objects::JValue::Int(0)],
    )?;
    println!("permissions requested?");
    Ok(false)
}
use std::sync::{
    RwLock,
    mpsc::{SyncSender, sync_channel},
};

lazy_static::lazy_static! {
    static ref PERMISSION_SENDER: RwLock<Option<SyncSender<RequestPermissionResult>>> = Default::default();
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestPermissionResult {
    pub granted: bool,
    pub permission: String,
}

pub async fn request_permission() -> Result<bool, jni::errors::Error> {
    let receiver = {
        let mut sender_guard = PERMISSION_SENDER.write().unwrap();
        let (sender, receiver) = sync_channel(1);
        sender_guard.replace(sender);
        receiver
    };
    let res = request_permission_inner()?;
    if res {
        Ok(true)
    } else {
        let result = receiver.recv().unwrap();
        Ok(result.granted)
    }
}

pub(crate) fn on_request_permission_result(
    permission: String,
    granted: bool,
) -> Result<(), jni::errors::Error> {
    let sender = PERMISSION_SENDER.read().unwrap();
    if let Some(sender) = sender.as_ref() {
        let permission_result = RequestPermissionResult {
            granted,
            permission,
        };
        let res = sender.try_send(permission_result);
        if let Err(err) = res {
            println!(
                "Received permission result but no one is listening: {:?}",
                err
            );
        }
    } else {
        println!("Received permission result but no one is listening");
    }
    Ok(())
}

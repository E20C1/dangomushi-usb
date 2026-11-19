use jni::{
    jni_sig, jni_str,
    objects::{JObject, JString, JValue},
    refs::Global,
    vm::JavaVM,
    Env,
};
use std::path::PathBuf;
use winit::platform::android::activity::AndroidApp;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageVolumeInfo {
    pub description: String,
    pub uuid: Option<String>,
    pub state: String,
    pub removable: bool,
    pub directory: PathBuf,
}

pub struct AndroidBridge {
    app: AndroidApp,
}

impl AndroidBridge {
    pub fn new(app: AndroidApp) -> Self {
        Self { app }
    }

    pub fn is_all_files_access_granted(&self) -> Result<bool, String> {
        self.with_activity(|env, _activity| {
            env.call_static_method(
                jni_str!("android/os/Environment"),
                jni_str!("isExternalStorageManager"),
                jni_sig!("()Z"),
                &[],
            )?
            .z()
        })
    }

    pub fn storage_volumes(&self) -> Result<Vec<StorageVolumeInfo>, String> {
        self.with_activity(|env, activity| {
            let storage_service = JString::from_str(env, "storage")?;
            let storage_manager = env
                .call_method(
                    activity,
                    jni_str!("getSystemService"),
                    jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                    &[JValue::Object(storage_service.as_ref())],
                )?
                .l()?;

            let volumes = env
                .call_method(
                    &storage_manager,
                    jni_str!("getStorageVolumes"),
                    jni_sig!("()Ljava/util/List;"),
                    &[],
                )?
                .l()?;

            let count = env
                .call_method(&volumes, jni_str!("size"), jni_sig!("()I"), &[])?
                .i()?;

            let mut result = Vec::new();
            for index in 0..count {
                let volume = env
                    .call_method(
                        &volumes,
                        jni_str!("get"),
                        jni_sig!("(I)Ljava/lang/Object;"),
                        &[JValue::from(index)],
                    )?
                    .l()?;

                let removable = env
                    .call_method(&volume, jni_str!("isRemovable"), jni_sig!("()Z"), &[])?
                    .z()?;

                let state_object = env
                    .call_method(
                        &volume,
                        jni_str!("getState"),
                        jni_sig!("()Ljava/lang/String;"),
                        &[],
                    )?
                    .l()?;
                let state = object_string(env, state_object)?;

                let description_object = env
                    .call_method(
                        &volume,
                        jni_str!("getDescription"),
                        jni_sig!("(Landroid/content/Context;)Ljava/lang/String;"),
                        &[JValue::Object(activity)],
                    )?
                    .l()?;
                let description = object_string(env, description_object)?;

                let uuid_object = env
                    .call_method(
                        &volume,
                        jni_str!("getUuid"),
                        jni_sig!("()Ljava/lang/String;"),
                        &[],
                    )?
                    .l()?;
                let uuid = if uuid_object.is_null() {
                    None
                } else {
                    Some(object_string(env, uuid_object)?)
                };

                let directory_object = env
                    .call_method(
                        &volume,
                        jni_str!("getDirectory"),
                        jni_sig!("()Ljava/io/File;"),
                        &[],
                    )?
                    .l()?;
                if directory_object.is_null() {
                    continue;
                }

                let path_object = env
                    .call_method(
                        &directory_object,
                        jni_str!("getAbsolutePath"),
                        jni_sig!("()Ljava/lang/String;"),
                        &[],
                    )?
                    .l()?;
                let directory = object_string(env, path_object)?;

                result.push(StorageVolumeInfo {
                    description,
                    uuid,
                    state,
                    removable,
                    directory: PathBuf::from(directory),
                });
            }

            Ok(result)
        })
    }

    fn with_activity<T>(
        &self,
        operation: impl FnOnce(&mut Env<'_>, &JObject<'_>) -> jni::errors::Result<T>,
    ) -> Result<T, String> {
        let vm = JavaVM::singleton()
            .unwrap_or_else(|_| unsafe { JavaVM::from_raw(self.app.vm_as_ptr().cast()) });

        vm.attach_current_thread(|env| {
            let raw_activity = self.app.activity_as_ptr() as jni::sys::jobject;
            let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity)? };
            operation(env, activity.as_ref())
        })
        .map_err(|error| format!("{error:?}"))
    }
}

fn object_string(env: &mut Env<'_>, object: JObject<'_>) -> jni::errors::Result<String> {
    let string = env.cast_local::<JString>(object)?;
    string.try_to_string(env)
}

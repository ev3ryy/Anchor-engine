use std::ffi::{CStr, c_char, c_void};

use ash::{Entry, Instance, ext, vk};
use raw_window_handle::HasDisplayHandle;

use crate::{Error, supports_vulkan_1_3};

const VALIDATION_LAYER: &CStr = c"VK_LAYER_KHRONOS_validation";

pub struct DebugState {
    pub loader: ext::debug_utils::Instance,
    pub messenger: vk::DebugUtilsMessengerEXT,
}

pub struct InstanceBundle {
    pub(crate) entry: Entry,
    pub instance: Instance,
    pub debug: Option<DebugState>,
}

impl Drop for InstanceBundle {
    fn drop(&mut self) {
        unsafe {
            if let Some(debug) = self.debug.take() {
                debug
                    .loader
                    .destroy_debug_utils_messenger(debug.messenger, None);
            }
            self.instance.destroy_instance(None);
        }
    }
}

pub fn create(display: &impl HasDisplayHandle) -> Result<InstanceBundle, Error> {
    let entry = unsafe { Entry::load() }.map_err(Error::Loading)?;

    let api_version =
        unsafe { entry.try_enumerate_instance_version() }?.unwrap_or(vk::API_VERSION_1_0);
    if !supports_vulkan_1_3(api_version) {
        return Err(Error::UnsupportedApiVersion(api_version));
    }

    let app_name = c"engine";
    let app_info = vk::ApplicationInfo::default()
        .application_name(app_name)
        .application_version(vk::make_api_version(0, 0, 1, 0))
        .engine_name(app_name)
        .engine_version(vk::make_api_version(0, 0, 1, 0))
        .api_version(vk::API_VERSION_1_3);

    let enable_validation = cfg!(debug_assertions) && has_validation_layer(&entry)?;

    let display_handle = display
        .display_handle()
        .map_err(Error::WindowHandle)?
        .as_raw();
    let mut extension_names = ash_window::enumerate_required_extensions(display_handle)?.to_vec();
    if enable_validation {
        extension_names.push(ext::debug_utils::NAME.as_ptr());
    }

    let layer_names: Vec<*const c_char> = if enable_validation {
        vec![VALIDATION_LAYER.as_ptr()]
    } else {
        Vec::new()
    };

    let mut debug_info = debug_messenger_create_info();
    let mut create_info = vk::InstanceCreateInfo::default()
        .application_info(&app_info)
        .enabled_extension_names(&extension_names)
        .enabled_layer_names(&layer_names);

    if enable_validation {
        create_info = create_info.push_next(&mut debug_info);
    }

    let instance = unsafe { entry.create_instance(&create_info, None) }?;

    let debug = if enable_validation {
        let loader = ext::debug_utils::Instance::new(&entry, &instance);
        match unsafe { loader.create_debug_utils_messenger(&debug_messenger_create_info(), None) } {
            Ok(messenger) => Some(DebugState { loader, messenger }),
            Err(err) => {
                unsafe { instance.destroy_instance(None) };
                return Err(err.into());
            }
        }
    } else {
        None
    };

    Ok(InstanceBundle {
        entry,
        instance,
        debug,
    })
}

fn has_validation_layer(entry: &Entry) -> Result<bool, Error> {
    let layers = unsafe { entry.enumerate_instance_layer_properties() }?;
    Ok(layers.iter().any(|layer| {
        layer
            .layer_name_as_c_str()
            .is_ok_and(|name| name == VALIDATION_LAYER)
    }))
}

fn debug_messenger_create_info<'a>() -> vk::DebugUtilsMessengerCreateInfoEXT<'a> {
    vk::DebugUtilsMessengerCreateInfoEXT::default()
        .message_severity(
            vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR,
        )
        .message_type(
            vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
        )
        .pfn_user_callback(Some(debug_callback))
}

unsafe extern "system" fn debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    _user_data: *mut c_void,
) -> vk::Bool32 {
    if callback_data.is_null() {
        return vk::FALSE;
    }
    let message_ptr = unsafe { (*callback_data).p_message };
    if message_ptr.is_null() {
        return vk::FALSE;
    }
    let message = unsafe { CStr::from_ptr(message_ptr) };
    let level = if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) {
        "ERROR"
    } else if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::WARNING) {
        "WARNING"
    } else {
        "INFO"
    };
    eprintln!("[Vulkan {level}] ({message_type:?}) {message:?}");
    vk::FALSE
}

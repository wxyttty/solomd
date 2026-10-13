//! Apple's own "rate this app" sheet (StoreKit `SKStoreReviewController`).
//!
//! App Store builds ask for a rating once, after SoloMD has been opened on
//! several different days (the same rule other builds use for the GitHub-star
//! prompt, see `src/lib/star-prompt.ts`). There is deliberately no custom
//! "do you like SoloMD?" step in front of it: Apple's guidelines want the
//! system sheet itself, and the system decides whether to show it at all
//! (at most three times a year per user, never more often than it allows).
//! Elsewhere the command is a no-op.

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[link(name = "StoreKit", kind = "framework")]
extern "C" {}

#[cfg(target_os = "macos")]
unsafe fn present() {
    use objc2::msg_send;
    use objc2::runtime::AnyClass;
    if let Some(cls) = AnyClass::get("SKStoreReviewController") {
        let _: () = msg_send![cls, requestReview];
    }
}

#[cfg(target_os = "ios")]
#[link(name = "UIKit", kind = "framework")]
extern "C" {}

#[cfg(target_os = "ios")]
unsafe fn present() {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    // iOS 14+: the sheet needs the window scene it belongs to — the one in
    // the foreground (UISceneActivationStateForegroundActive == 0).
    let (Some(app_cls), Some(scene_cls), Some(review_cls)) = (
        AnyClass::get("UIApplication"),
        AnyClass::get("UIWindowScene"),
        AnyClass::get("SKStoreReviewController"),
    ) else {
        return;
    };
    let app: *mut AnyObject = msg_send![app_cls, sharedApplication];
    if app.is_null() {
        return;
    }
    let scenes: *mut AnyObject = msg_send![app, connectedScenes];
    let all: *mut AnyObject = msg_send![scenes, allObjects];
    let count: usize = msg_send![all, count];
    for i in 0..count {
        let scene: *mut AnyObject = msg_send![all, objectAtIndex: i];
        let is_window_scene: Bool = msg_send![scene, isKindOfClass: scene_cls];
        let state: isize = msg_send![scene, activationState];
        if is_window_scene.as_bool() && state == 0 {
            let _: () = msg_send![review_cls, requestReviewInScene: scene];
            return;
        }
    }
}

/// Ask the system to show the App Store rating sheet (it may decline).
#[tauri::command]
pub fn request_store_review(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        app.run_on_main_thread(|| unsafe { present() })
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    let _ = app;
    Ok(())
}

//! Mac only: fill the menu's "About Cache Deck" window with the app's icon,
//! version and developer details, also when the program runs outside an app package.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{AllocAnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSColor, NSFont, NSFontAttributeName, NSForegroundColorAttributeName, NSImage, NSLinkAttributeName,
    NSMutableParagraphStyle, NSParagraphStyleAttributeName, NSTextAlignment,
};
use objc2_foundation::{NSData, NSDictionary, NSMutableAttributedString, NSRange, NSString, NSURL};
use std::cell::OnceCell;

use crate::app::{DEVELOPER, GITHUB_URL};

define_class!(
    // SAFETY: NSObject has no subclassing requirements and this class has no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CacheDeckAboutTarget"]
    struct AboutTarget;

    impl AboutTarget {
        #[unsafe(method(showCacheDeckAbout:))]
        fn show_about(&self, _sender: Option<&AnyObject>) {
            show_panel(self.mtm());
        }
    }

    unsafe impl NSObjectProtocol for AboutTarget {}
);

impl AboutTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

thread_local! {
    static TARGET: OnceCell<Retained<AboutTarget>> = const { OnceCell::new() };
}

fn credits() -> Retained<NSMutableAttributedString> {
    let link_text = GITHUB_URL.trim_start_matches("https://");
    let text = format!("A desktop client for Redis and Valkey\n\nDeveloped by {DEVELOPER}\nGitHub: {link_text}");
    let ns = NSString::from_str(&text);
    let out = NSMutableAttributedString::from_nsstring(&ns);
    let utf16_len = |s: &str| s.encode_utf16().count();
    let all = NSRange::new(0, utf16_len(&text));
    unsafe {
        let style = NSMutableParagraphStyle::new();
        style.setAlignment(NSTextAlignment::Center);
        out.addAttribute_value_range(NSParagraphStyleAttributeName, &style, all);
        out.addAttribute_value_range(NSFontAttributeName, &NSFont::systemFontOfSize(11.0), all);
        out.addAttribute_value_range(NSForegroundColorAttributeName, &NSColor::labelColor(), all);

        let dev_start = utf16_len(&text[..text.find(DEVELOPER).unwrap()]);
        out.addAttribute_value_range(
            NSFontAttributeName,
            &NSFont::boldSystemFontOfSize(11.0),
            NSRange::new(dev_start, utf16_len(DEVELOPER)),
        );
        if let Some(url) = NSURL::URLWithString(&NSString::from_str(GITHUB_URL)) {
            let link_start = utf16_len(&text[..text.rfind(link_text).unwrap()]);
            out.addAttribute_value_range(
                NSLinkAttributeName,
                &url,
                NSRange::new(link_start, utf16_len(link_text)),
            );
        }
    }
    out
}

/// Open the standard Mac About window with Cache Deck's details.
pub fn show_panel(mtm: MainThreadMarker) {
    let app = NSApplication::sharedApplication(mtm);
    let icon_bytes = include_bytes!("../../assets/app-icon/png/icon-512.png");
    let icon = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(icon_bytes));
    let name = NSString::from_str("Cache Deck");
    let version = NSString::from_str(env!("CARGO_PKG_VERSION"));
    let empty = NSString::from_str("");
    let copyright = NSString::from_str("© 2026 Shaon Dey");
    let credits = credits();

    let k_name = NSString::from_str("ApplicationName");
    let k_icon = NSString::from_str("ApplicationIcon");
    let k_appver = NSString::from_str("ApplicationVersion");
    let k_ver = NSString::from_str("Version");
    let k_credits = NSString::from_str("Credits");
    let k_copy = NSString::from_str("Copyright");

    let mut keys: Vec<&NSString> = vec![&k_name, &k_appver, &k_ver, &k_credits, &k_copy];
    let mut vals: Vec<&AnyObject> = vec![&name, &version, &empty, &credits, &copyright];
    if let Some(i) = &icon {
        keys.push(&k_icon);
        vals.push(i);
    }
    let dict = NSDictionary::from_slices(&keys, &vals);
    unsafe {
        let dict: &NSDictionary<NSString, AnyObject> = &dict;
        let _: () = msg_send![&*app, orderFrontStandardAboutPanelWithOptions: dict];
    }
}

/// Point the menu's "About Cache Deck" item at our panel. Safe to call every frame; it
/// only acts once the menu exists and until it has been changed.
pub fn install() -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let app = NSApplication::sharedApplication(mtm);
    let Some(menu) = app.mainMenu() else { return false };
    let Some(app_item) = menu.itemAtIndex(0) else {
        return false;
    };
    let Some(sub) = app_item.submenu() else { return false };
    let Some(about) = sub.itemAtIndex(0) else { return false };
    let target = TARGET.with(|t| t.get_or_init(|| AboutTarget::new(mtm)).clone());
    unsafe {
        about.setTarget(Some(&target));
        about.setAction(Some(sel!(showCacheDeckAbout:)));
    }
    true
}

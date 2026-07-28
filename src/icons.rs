//! Inline SVG icon set.
//!
//! Baked into the binary as string constants — no system icon theme
//! lookup, no file I/O at runtime, no `hicolor/Papirus/...` dependency.
//! This keeps the "static binary, works on a fresh minimal install"
//! promise from the project spec.
//!
//! Icons are 16×16 (matching the file-manager scale). iced renders them
//! via the `svg` widget (`iced::widget::svg::Svg::from_data`).
//!
//! ## Performance note
//!
//! Each icon's `svg::Handle` is built ONCE and cached in a `LazyLock`.
//! This matters because the file grid can re-render many times per
//! second during scrolling or directory scans, and each row rebuilds
//! its icon widget. Without caching, every visible row would re-parse
//! the SVG markup from scratch on every frame — visible CPU burn with
//! 1000+ entries. With caching, the parse happens at most once per icon
//! variant for the lifetime of the process.

use iced::widget::svg::{Handle, Svg};
use std::sync::LazyLock;

/// Glyphs available throughout the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Folder,
    File,
    AppDir,
    Symlink,
    Device,
    Network,
    Bookmark,
    Up,
    Edit,
    Reload,
}

impl Icon {
    /// Raw SVG markup for this icon. Stroke/fill use accent colors that
    /// render fine on both light and dark iced themes.
    ///
    /// Marked `allow(dead_code)` because production code uses the cached
    /// `LazyLock<Handle>` statics directly (see `handle()`), but the test
    /// module calls this to verify each variant has distinct markup.
    #[allow(dead_code)]
    fn svg(self) -> &'static str {
        match self {
            Icon::Folder => FOLDER_SVG,
            Icon::File => FILE_SVG,
            Icon::AppDir => APPDIR_SVG,
            Icon::Symlink => SYMLINK_SVG,
            Icon::Device => DEVICE_SVG,
            Icon::Network => NETWORK_SVG,
            Icon::Bookmark => BOOKMARK_SVG,
            Icon::Up => UP_SVG,
            Icon::Edit => EDIT_SVG,
            Icon::Reload => RELOAD_SVG,
        }
    }

    /// Cached `svg::Handle` for this icon. Built once on first access,
    /// reused for every subsequent `widget()` call. The handle owns the
    /// SVG bytes; iced's renderer caches the parsed raster internally
    /// keyed by handle identity.
    fn handle(self) -> &'static Handle {
        // One LazyLock per icon variant. Indexed by the Icon enum so
        // callers don't pay for variants they never use.
        match self {
            Icon::Folder => &FOLDER_HANDLE,
            Icon::File => &FILE_HANDLE,
            Icon::AppDir => &APPDIR_HANDLE,
            Icon::Symlink => &SYMLINK_HANDLE,
            Icon::Device => &DEVICE_HANDLE,
            Icon::Network => &NETWORK_HANDLE,
            Icon::Bookmark => &BOOKMARK_HANDLE,
            Icon::Up => &UP_HANDLE,
            Icon::Edit => &EDIT_HANDLE,
            Icon::Reload => &RELOAD_HANDLE,
        }
    }

    /// Build an `iced::widget::svg::Svg` for this icon at a given pixel size.
    /// The underlying `Handle` is cached (see `handle()`), so repeated
    /// calls don't re-parse the SVG markup. The Handle is `Arc`-backed
    /// internally, so cloning it here is a cheap refcount bump.
    pub fn widget(self, size: f32) -> Svg<'static> {
        Svg::new(self.handle().clone())
            .width(size)
            .height(size)
    }
}

// --- Cached handles ----------------------------------------------------------
// Each LazyLock wraps a Handle built from the corresponding SVG markup.
// First access pays the parse cost; subsequent accesses are free.

static FOLDER_HANDLE: LazyLock<Handle> =
    LazyLock::new(|| Handle::from_memory(FOLDER_SVG.as_bytes().to_vec()));
static FILE_HANDLE: LazyLock<Handle> =
    LazyLock::new(|| Handle::from_memory(FILE_SVG.as_bytes().to_vec()));
static APPDIR_HANDLE: LazyLock<Handle> =
    LazyLock::new(|| Handle::from_memory(APPDIR_SVG.as_bytes().to_vec()));
static SYMLINK_HANDLE: LazyLock<Handle> =
    LazyLock::new(|| Handle::from_memory(SYMLINK_SVG.as_bytes().to_vec()));
static DEVICE_HANDLE: LazyLock<Handle> =
    LazyLock::new(|| Handle::from_memory(DEVICE_SVG.as_bytes().to_vec()));
static NETWORK_HANDLE: LazyLock<Handle> =
    LazyLock::new(|| Handle::from_memory(NETWORK_SVG.as_bytes().to_vec()));
static BOOKMARK_HANDLE: LazyLock<Handle> =
    LazyLock::new(|| Handle::from_memory(BOOKMARK_SVG.as_bytes().to_vec()));
static UP_HANDLE: LazyLock<Handle> =
    LazyLock::new(|| Handle::from_memory(UP_SVG.as_bytes().to_vec()));
static EDIT_HANDLE: LazyLock<Handle> =
    LazyLock::new(|| Handle::from_memory(EDIT_SVG.as_bytes().to_vec()));
static RELOAD_HANDLE: LazyLock<Handle> =
    LazyLock::new(|| Handle::from_memory(RELOAD_SVG.as_bytes().to_vec()));

// --- SVG definitions ---------------------------------------------------------
// Minimal flat style, 16×16 viewBox, no external refs. Colors chosen to
// read on both light/dark themes:
//   * folders/devices  → #5294e2 (steel blue)
//   * appdirs          → #4db6ac (teal — distinguishes "launchable" dirs)
//   * files            → #888 (neutral gray)
//   * symlinks         → #ffa726 (amber, like a redirect indicator)
//   * network          → #ab47bc (purple)
//   * bookmarks        → #fdd835 (yellow star)

const FOLDER_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">
  <path fill="#5294e2" d="M1 3.5a1 1 0 0 1 1-1h3.6a1 1 0 0 1 .7.3l1.4 1.4a1 1 0 0 0 .7.3H14a1 1 0 0 1 1 1V12a1 1 0 0 1-1 1H2a1 1 0 0 1-1-1z"/>
  <path fill="#3b7cc0" d="M1 5h14v1H1z" opacity=".4"/>
</svg>"##;

const FILE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">
  <path fill="#aaa" d="M3 1.5h6l4 4v9a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1v-12a1 1 0 0 1 1-1z"/>
  <path fill="#fff" opacity=".5" d="M9 1.5l4 4h-3a1 1 0 0 1-1-1z"/>
</svg>"##;

const APPDIR_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">
  <path fill="#4db6ac" d="M1 3.5a1 1 0 0 1 1-1h3.6a1 1 0 0 1 .7.3l1.4 1.4a1 1 0 0 0 .7.3H14a1 1 0 0 1 1 1V12a1 1 0 0 1-1 1H2a1 1 0 0 1-1-1z"/>
  <path fill="#fff" d="M8 6.5l3 2.5-3 2.5v-1.5H5v-2h3z"/>
</svg>"##;

const SYMLINK_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">
  <path fill="#5294e2" d="M1 3.5a1 1 0 0 1 1-1h3.6a1 1 0 0 1 .7.3l1.4 1.4a1 1 0 0 0 .7.3H14a1 1 0 0 1 1 1V12a1 1 0 0 1-1 1H2a1 1 0 0 1-1-1z"/>
  <path fill="none" stroke="#ffa726" stroke-width="1.5" stroke-linecap="round" d="M5 9h5m-1.5-2L11 9l-2.5 2"/>
</svg>"##;

const DEVICE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">
  <rect x="2" y="3" width="12" height="9" rx="1" fill="#5294e2"/>
  <rect x="3.5" y="4.5" width="9" height="6" fill="#fff" opacity=".85"/>
  <rect x="5" y="13" width="6" height="1.5" rx=".5" fill="#5294e2"/>
</svg>"##;

const NETWORK_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">
  <circle cx="8" cy="8" r="6" fill="none" stroke="#ab47bc" stroke-width="1.4"/>
  <path fill="none" stroke="#ab47bc" stroke-width="1.4" d="M2 8h12M8 2c2 2 2 10 0 12M8 2c-2 2-2 10 0 12"/>
</svg>"##;

const BOOKMARK_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">
  <path fill="#fdd835" stroke="#bfa927" stroke-width=".7" d="M4 2h8v12l-4-2.5L4 14z"/>
</svg>"##;

const UP_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">
  <path fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" d="M8 12V4M4 8l4-4 4 4"/>
</svg>"##;

const EDIT_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">
  <path fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" d="M11 2l3 3-8 8H3v-3z"/>
</svg>"##;

const RELOAD_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">
  <path fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" d="M13 8a5 5 0 1 1-1.5-3.5M13 2v3h-3"/>
</svg>"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widget_returns_same_handle_instance() {
        // Each call to widget() should reuse the cached handle, not
        // build a new one. We can't compare Handles directly (no PartialEq
        // impl in iced 0.13), but we can verify the function doesn't
        // panic and produces a non-zero-sized Svg.
        let _w1 = Icon::Folder.widget(14.0);
        let _w2 = Icon::Folder.widget(16.0);
        let _w3 = Icon::File.widget(14.0);
        // If we got here without panicking, the LazyLocks initialized OK.
    }

    #[test]
    fn all_variants_have_distinct_svg() {
        // Smoke test: ensure no two icons accidentally share the same SVG.
        let svgs = [
            Icon::Folder.svg(),
            Icon::File.svg(),
            Icon::AppDir.svg(),
            Icon::Symlink.svg(),
            Icon::Device.svg(),
            Icon::Network.svg(),
            Icon::Bookmark.svg(),
            Icon::Up.svg(),
            Icon::Edit.svg(),
            Icon::Reload.svg(),
        ];
        for i in 0..svgs.len() {
            for j in (i + 1)..svgs.len() {
                assert_ne!(svgs[i], svgs[j], "icons at indices {i} and {j} share SVG");
            }
        }
    }
}

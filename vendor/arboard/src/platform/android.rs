//! Android: no system clipboard backend yet. The shell's JNI clipboard bridge is a later layer;
//! every operation reports the clipboard as unsupported.

use crate::common::Error;
#[cfg(feature = "image-data")]
use crate::common::ImageData;
use std::borrow::Cow;
use std::path::{Path, PathBuf};

pub struct Clipboard;

impl Clipboard {
    pub fn new() -> Result<Self, Error> {
        Err(Error::ClipboardNotSupported)
    }
}

pub struct Get<'a> {
    _clipboard: &'a mut Clipboard,
}

impl<'a> Get<'a> {
    pub fn new(clipboard: &'a mut Clipboard) -> Self {
        Self { _clipboard: clipboard }
    }
    pub fn text(self) -> Result<String, Error> {
        Err(Error::ClipboardNotSupported)
    }
    #[cfg(feature = "image-data")]
    pub fn image(self) -> Result<ImageData<'static>, Error> {
        Err(Error::ClipboardNotSupported)
    }
    pub fn html(self) -> Result<String, Error> {
        Err(Error::ClipboardNotSupported)
    }
    pub fn file_list(self) -> Result<Vec<PathBuf>, Error> {
        Err(Error::ClipboardNotSupported)
    }
}

pub struct Set<'a> {
    _clipboard: &'a mut Clipboard,
}

impl<'a> Set<'a> {
    pub fn new(clipboard: &'a mut Clipboard) -> Self {
        Self { _clipboard: clipboard }
    }
    pub fn text<'b, T: Into<Cow<'b, str>>>(self, _text: T) -> Result<(), Error> {
        Err(Error::ClipboardNotSupported)
    }
    pub fn html<'b, T: Into<Cow<'b, str>>>(self, _html: T, _alt_text: Option<T>) -> Result<(), Error> {
        Err(Error::ClipboardNotSupported)
    }
    #[cfg(feature = "image-data")]
    pub fn image(self, _image: ImageData) -> Result<(), Error> {
        Err(Error::ClipboardNotSupported)
    }
    pub fn file_list(self, _file_list: &[impl AsRef<Path>]) -> Result<(), Error> {
        Err(Error::ClipboardNotSupported)
    }
}

pub struct Clear<'a> {
    _clipboard: &'a mut Clipboard,
}

impl<'a> Clear<'a> {
    pub fn new(clipboard: &'a mut Clipboard) -> Self {
        Self { _clipboard: clipboard }
    }
    pub fn clear(self) -> Result<(), Error> {
        Err(Error::ClipboardNotSupported)
    }
}

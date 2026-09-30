use crate::CodecError;
use core::{fmt::Write, str::FromStr};
pub type Text = heapless::String<128>;
pub type Cells = heapless::Vec<Text, 16>;
pub trait Cell: Sized {
    fn read(text: &str) -> Result<Self, CodecError>;
    fn write(&self, text: &mut Text) -> Result<(), CodecError>;
}
pub fn cell<T: Cell>(text: &str) -> Result<Option<T>, CodecError> {
    if text.is_empty() {
        Ok(None)
    } else {
        T::read(text).map(Some)
    }
}
pub fn push_cell<T: Cell>(cells: &mut Cells, value: Option<&T>) -> Result<(), CodecError> {
    let mut text = Text::new();
    if let Some(value) = value {
        value.write(&mut text)?;
    }
    cells.push(text).map_err(|_| CodecError::Size)
}
macro_rules! integer {
    ($t:ty) => {
        impl Cell for $t {
            fn read(text: &str) -> Result<Self, CodecError> {
                let value = <$t>::from_str(text).map_err(|_| CodecError::Decode)?;
                let mut canonical = Text::new();
                write!(&mut canonical, "{}", value).map_err(|_| CodecError::Size)?;
                if canonical != text {
                    return Err(CodecError::Decode);
                }
                Ok(value)
            }
            fn write(&self, text: &mut Text) -> Result<(), CodecError> {
                write!(text, "{}", self).map_err(|_| CodecError::Size)
            }
        }
    };
}
integer!(u32);
integer!(i32);
impl Cell for bool {
    fn read(text: &str) -> Result<Self, CodecError> {
        match text {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(CodecError::Decode),
        }
    }
    fn write(&self, text: &mut Text) -> Result<(), CodecError> {
        text.push_str(if *self { "true" } else { "false" })
            .map_err(|_| CodecError::Size)
    }
}
impl Cell for f32 {
    fn read(text: &str) -> Result<Self, CodecError> {
        text.parse().map_err(|_| CodecError::Decode)
    }
    fn write(&self, text: &mut Text) -> Result<(), CodecError> {
        text.push_str(ryu::Buffer::new().format(*self))
            .map_err(|_| CodecError::Size)
    }
}
impl<const N: usize> Cell for heapless::String<N> {
    fn read(text: &str) -> Result<Self, CodecError> {
        Self::from_str(text).map_err(|_| CodecError::Decode)
    }
    fn write(&self, text: &mut Text) -> Result<(), CodecError> {
        text.push_str(self).map_err(|_| CodecError::Size)
    }
}

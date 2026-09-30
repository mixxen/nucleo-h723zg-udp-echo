pub mod messaging_ {
    pub mod v1_ {
        #[derive(Debug, Default, PartialEq, Clone, Copy)]
        pub struct Empty {}
        impl Empty {}
        impl ::micropb::MessageDecode for Empty {
            fn decode<IMPL_MICROPB_READ: ::micropb::PbRead>(
                &mut self,
                decoder: &mut ::micropb::PbDecoder<IMPL_MICROPB_READ>,
                len: usize,
            ) -> Result<(), ::micropb::DecodeError<IMPL_MICROPB_READ::Error>> {
                use ::micropb::{PbBytes, PbString, PbVec, PbMap, FieldDecode};
                let before = decoder.bytes_read();
                while decoder.bytes_read() - before < len {
                    let tag = decoder.decode_tag()?;
                    match tag.field_num() {
                        0 => return Err(::micropb::DecodeError::ZeroField),
                        _ => {
                            decoder.skip_wire_value(tag.wire_type())?;
                        }
                    }
                }
                Ok(())
            }
        }
        impl ::micropb::MessageEncode for Empty {
            const MAX_SIZE: ::core::result::Result<usize, &'static str> = 'msg: {
                let mut max_size = 0;
                ::core::result::Result::Ok(max_size)
            };
            fn encode<IMPL_MICROPB_WRITE: ::micropb::PbWrite>(
                &self,
                encoder: &mut ::micropb::PbEncoder<IMPL_MICROPB_WRITE>,
            ) -> Result<(), IMPL_MICROPB_WRITE::Error> {
                use ::micropb::{PbMap, FieldEncode};
                Ok(())
            }
            fn compute_size(&self) -> usize {
                use ::micropb::{PbMap, FieldEncode};
                let mut size = 0;
                size
            }
        }
        #[derive(Debug, Default, PartialEq, Clone)]
        pub struct DeviceInfo {
            pub r#firmware_version: ::core::option::Option<::heapless::String<32>>,
            pub r#encoding: ::core::option::Option<Encoding>,
        }
        impl DeviceInfo {
            /// Return a reference to `firmware_version` as an `Option`
            #[inline]
            pub fn r#firmware_version(
                &self,
            ) -> ::core::option::Option<&::heapless::String<32>> {
                self.r#firmware_version.as_ref()
            }
            /// Set the value and presence of `firmware_version`
            #[inline]
            pub fn set_firmware_version(
                &mut self,
                value: ::heapless::String<32>,
            ) -> &mut Self {
                self.r#firmware_version = ::core::option::Option::Some(value.into());
                self
            }
            /// Return a mutable reference to `firmware_version` as an `Option`
            #[inline]
            pub fn mut_firmware_version(
                &mut self,
            ) -> ::core::option::Option<&mut ::heapless::String<32>> {
                self.r#firmware_version.as_mut()
            }
            /// Clear the presence of `firmware_version`
            #[inline]
            pub fn clear_firmware_version(&mut self) -> &mut Self {
                self.r#firmware_version = ::core::option::Option::None;
                self
            }
            /// Take the value of `firmware_version` and clear its presence
            #[inline]
            pub fn take_firmware_version(
                &mut self,
            ) -> ::core::option::Option<::heapless::String<32>> {
                self.r#firmware_version.take()
            }
            /// Builder method that sets the value of `firmware_version`. Useful for initializing the message.
            #[inline]
            pub fn init_firmware_version(
                mut self,
                value: ::heapless::String<32>,
            ) -> Self {
                self.set_firmware_version(value);
                self
            }
            /// Return a reference to `encoding` as an `Option`
            #[inline]
            pub fn r#encoding(&self) -> ::core::option::Option<&Encoding> {
                self.r#encoding.as_ref()
            }
            /// Set the value and presence of `encoding`
            #[inline]
            pub fn set_encoding(&mut self, value: Encoding) -> &mut Self {
                self.r#encoding = ::core::option::Option::Some(value.into());
                self
            }
            /// Return a mutable reference to `encoding` as an `Option`
            #[inline]
            pub fn mut_encoding(&mut self) -> ::core::option::Option<&mut Encoding> {
                self.r#encoding.as_mut()
            }
            /// Clear the presence of `encoding`
            #[inline]
            pub fn clear_encoding(&mut self) -> &mut Self {
                self.r#encoding = ::core::option::Option::None;
                self
            }
            /// Take the value of `encoding` and clear its presence
            #[inline]
            pub fn take_encoding(&mut self) -> ::core::option::Option<Encoding> {
                self.r#encoding.take()
            }
            /// Builder method that sets the value of `encoding`. Useful for initializing the message.
            #[inline]
            pub fn init_encoding(mut self, value: Encoding) -> Self {
                self.set_encoding(value);
                self
            }
        }
        impl ::micropb::MessageDecode for DeviceInfo {
            fn decode<IMPL_MICROPB_READ: ::micropb::PbRead>(
                &mut self,
                decoder: &mut ::micropb::PbDecoder<IMPL_MICROPB_READ>,
                len: usize,
            ) -> Result<(), ::micropb::DecodeError<IMPL_MICROPB_READ::Error>> {
                use ::micropb::{PbBytes, PbString, PbVec, PbMap, FieldDecode};
                let before = decoder.bytes_read();
                while decoder.bytes_read() - before < len {
                    let tag = decoder.decode_tag()?;
                    match tag.field_num() {
                        0 => return Err(::micropb::DecodeError::ZeroField),
                        1u32 => {
                            let mut_ref = &mut *self
                                .r#firmware_version
                                .get_or_insert_with(::core::default::Default::default);
                            {
                                decoder
                                    .decode_string(mut_ref, ::micropb::Presence::Explicit)?;
                            };
                        }
                        2u32 => {
                            let mut_ref = &mut *self
                                .r#encoding
                                .get_or_insert_with(::core::default::Default::default);
                            {
                                let val = decoder.decode_int32().map(|n| Encoding(n as _))?;
                                *mut_ref = val as _;
                            };
                        }
                        _ => {
                            decoder.skip_wire_value(tag.wire_type())?;
                        }
                    }
                }
                Ok(())
            }
        }
        impl ::micropb::MessageEncode for DeviceInfo {
            const MAX_SIZE: ::core::result::Result<usize, &'static str> = 'msg: {
                let mut max_size = 0;
                match ::micropb::const_map!(
                    ::core::result::Result::Ok(33usize), | size | size + 1usize
                ) {
                    ::core::result::Result::Ok(size) => {
                        max_size += size;
                    }
                    ::core::result::Result::Err(err) => {
                        break 'msg (::core::result::Result::<usize, _>::Err(err));
                    }
                }
                match ::micropb::const_map!(
                    ::core::result::Result::Ok(Encoding::_MAX_SIZE), | size | size +
                    1usize
                ) {
                    ::core::result::Result::Ok(size) => {
                        max_size += size;
                    }
                    ::core::result::Result::Err(err) => {
                        break 'msg (::core::result::Result::<usize, _>::Err(err));
                    }
                }
                ::core::result::Result::Ok(max_size)
            };
            fn encode<IMPL_MICROPB_WRITE: ::micropb::PbWrite>(
                &self,
                encoder: &mut ::micropb::PbEncoder<IMPL_MICROPB_WRITE>,
            ) -> Result<(), IMPL_MICROPB_WRITE::Error> {
                use ::micropb::{PbMap, FieldEncode};
                {
                    if let ::core::option::Option::Some(val_ref) = self
                        .r#firmware_version()
                    {
                        encoder.encode_varint32(10u32)?;
                        encoder.encode_string(val_ref)?;
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#encoding() {
                        encoder.encode_varint32(16u32)?;
                        encoder.encode_int32(val_ref.0 as _)?;
                    }
                }
                Ok(())
            }
            fn compute_size(&self) -> usize {
                use ::micropb::{PbMap, FieldEncode};
                let mut size = 0;
                {
                    if let ::core::option::Option::Some(val_ref) = self
                        .r#firmware_version()
                    {
                        size
                            += 1usize
                                + ::micropb::size::sizeof_len_record(val_ref.len());
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#encoding() {
                        size += 1usize + ::micropb::size::sizeof_int32(val_ref.0 as _);
                    }
                }
                size
            }
        }
        #[derive(Debug, Default, PartialEq, Clone, Copy)]
        pub struct Status {
            pub r#temperature_celsius: ::core::option::Option<f32>,
            pub r#validity: ::core::option::Option<Validity>,
            pub r#synthetic: ::core::option::Option<bool>,
        }
        impl Status {
            /// Return a reference to `temperature_celsius` as an `Option`
            #[inline]
            pub fn r#temperature_celsius(&self) -> ::core::option::Option<&f32> {
                self.r#temperature_celsius.as_ref()
            }
            /// Set the value and presence of `temperature_celsius`
            #[inline]
            pub fn set_temperature_celsius(&mut self, value: f32) -> &mut Self {
                self.r#temperature_celsius = ::core::option::Option::Some(value.into());
                self
            }
            /// Return a mutable reference to `temperature_celsius` as an `Option`
            #[inline]
            pub fn mut_temperature_celsius(
                &mut self,
            ) -> ::core::option::Option<&mut f32> {
                self.r#temperature_celsius.as_mut()
            }
            /// Clear the presence of `temperature_celsius`
            #[inline]
            pub fn clear_temperature_celsius(&mut self) -> &mut Self {
                self.r#temperature_celsius = ::core::option::Option::None;
                self
            }
            /// Take the value of `temperature_celsius` and clear its presence
            #[inline]
            pub fn take_temperature_celsius(&mut self) -> ::core::option::Option<f32> {
                self.r#temperature_celsius.take()
            }
            /// Builder method that sets the value of `temperature_celsius`. Useful for initializing the message.
            #[inline]
            pub fn init_temperature_celsius(mut self, value: f32) -> Self {
                self.set_temperature_celsius(value);
                self
            }
            /// Return a reference to `validity` as an `Option`
            #[inline]
            pub fn r#validity(&self) -> ::core::option::Option<&Validity> {
                self.r#validity.as_ref()
            }
            /// Set the value and presence of `validity`
            #[inline]
            pub fn set_validity(&mut self, value: Validity) -> &mut Self {
                self.r#validity = ::core::option::Option::Some(value.into());
                self
            }
            /// Return a mutable reference to `validity` as an `Option`
            #[inline]
            pub fn mut_validity(&mut self) -> ::core::option::Option<&mut Validity> {
                self.r#validity.as_mut()
            }
            /// Clear the presence of `validity`
            #[inline]
            pub fn clear_validity(&mut self) -> &mut Self {
                self.r#validity = ::core::option::Option::None;
                self
            }
            /// Take the value of `validity` and clear its presence
            #[inline]
            pub fn take_validity(&mut self) -> ::core::option::Option<Validity> {
                self.r#validity.take()
            }
            /// Builder method that sets the value of `validity`. Useful for initializing the message.
            #[inline]
            pub fn init_validity(mut self, value: Validity) -> Self {
                self.set_validity(value);
                self
            }
            /// Return a reference to `synthetic` as an `Option`
            #[inline]
            pub fn r#synthetic(&self) -> ::core::option::Option<&bool> {
                self.r#synthetic.as_ref()
            }
            /// Set the value and presence of `synthetic`
            #[inline]
            pub fn set_synthetic(&mut self, value: bool) -> &mut Self {
                self.r#synthetic = ::core::option::Option::Some(value.into());
                self
            }
            /// Return a mutable reference to `synthetic` as an `Option`
            #[inline]
            pub fn mut_synthetic(&mut self) -> ::core::option::Option<&mut bool> {
                self.r#synthetic.as_mut()
            }
            /// Clear the presence of `synthetic`
            #[inline]
            pub fn clear_synthetic(&mut self) -> &mut Self {
                self.r#synthetic = ::core::option::Option::None;
                self
            }
            /// Take the value of `synthetic` and clear its presence
            #[inline]
            pub fn take_synthetic(&mut self) -> ::core::option::Option<bool> {
                self.r#synthetic.take()
            }
            /// Builder method that sets the value of `synthetic`. Useful for initializing the message.
            #[inline]
            pub fn init_synthetic(mut self, value: bool) -> Self {
                self.set_synthetic(value);
                self
            }
        }
        impl ::micropb::MessageDecode for Status {
            fn decode<IMPL_MICROPB_READ: ::micropb::PbRead>(
                &mut self,
                decoder: &mut ::micropb::PbDecoder<IMPL_MICROPB_READ>,
                len: usize,
            ) -> Result<(), ::micropb::DecodeError<IMPL_MICROPB_READ::Error>> {
                use ::micropb::{PbBytes, PbString, PbVec, PbMap, FieldDecode};
                let before = decoder.bytes_read();
                while decoder.bytes_read() - before < len {
                    let tag = decoder.decode_tag()?;
                    match tag.field_num() {
                        0 => return Err(::micropb::DecodeError::ZeroField),
                        1u32 => {
                            let mut_ref = &mut *self
                                .r#temperature_celsius
                                .get_or_insert_with(::core::default::Default::default);
                            {
                                let val = decoder.decode_float()?;
                                *mut_ref = val as _;
                            };
                        }
                        2u32 => {
                            let mut_ref = &mut *self
                                .r#validity
                                .get_or_insert_with(::core::default::Default::default);
                            {
                                let val = decoder.decode_int32().map(|n| Validity(n as _))?;
                                *mut_ref = val as _;
                            };
                        }
                        3u32 => {
                            let mut_ref = &mut *self
                                .r#synthetic
                                .get_or_insert_with(::core::default::Default::default);
                            {
                                let val = decoder.decode_bool()?;
                                *mut_ref = val as _;
                            };
                        }
                        _ => {
                            decoder.skip_wire_value(tag.wire_type())?;
                        }
                    }
                }
                Ok(())
            }
        }
        impl ::micropb::MessageEncode for Status {
            const MAX_SIZE: ::core::result::Result<usize, &'static str> = 'msg: {
                let mut max_size = 0;
                match ::micropb::const_map!(
                    ::core::result::Result::Ok(4usize), | size | size + 1usize
                ) {
                    ::core::result::Result::Ok(size) => {
                        max_size += size;
                    }
                    ::core::result::Result::Err(err) => {
                        break 'msg (::core::result::Result::<usize, _>::Err(err));
                    }
                }
                match ::micropb::const_map!(
                    ::core::result::Result::Ok(Validity::_MAX_SIZE), | size | size +
                    1usize
                ) {
                    ::core::result::Result::Ok(size) => {
                        max_size += size;
                    }
                    ::core::result::Result::Err(err) => {
                        break 'msg (::core::result::Result::<usize, _>::Err(err));
                    }
                }
                match ::micropb::const_map!(
                    ::core::result::Result::Ok(1usize), | size | size + 1usize
                ) {
                    ::core::result::Result::Ok(size) => {
                        max_size += size;
                    }
                    ::core::result::Result::Err(err) => {
                        break 'msg (::core::result::Result::<usize, _>::Err(err));
                    }
                }
                ::core::result::Result::Ok(max_size)
            };
            fn encode<IMPL_MICROPB_WRITE: ::micropb::PbWrite>(
                &self,
                encoder: &mut ::micropb::PbEncoder<IMPL_MICROPB_WRITE>,
            ) -> Result<(), IMPL_MICROPB_WRITE::Error> {
                use ::micropb::{PbMap, FieldEncode};
                {
                    if let ::core::option::Option::Some(val_ref) = self
                        .r#temperature_celsius()
                    {
                        encoder.encode_varint32(13u32)?;
                        encoder.encode_float(*val_ref)?;
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#validity() {
                        encoder.encode_varint32(16u32)?;
                        encoder.encode_int32(val_ref.0 as _)?;
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#synthetic() {
                        encoder.encode_varint32(24u32)?;
                        encoder.encode_bool(*val_ref)?;
                    }
                }
                Ok(())
            }
            fn compute_size(&self) -> usize {
                use ::micropb::{PbMap, FieldEncode};
                let mut size = 0;
                {
                    if let ::core::option::Option::Some(val_ref) = self
                        .r#temperature_celsius()
                    {
                        size += 1usize + 4;
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#validity() {
                        size += 1usize + ::micropb::size::sizeof_int32(val_ref.0 as _);
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#synthetic() {
                        size += 1usize + 1;
                    }
                }
                size
            }
        }
        #[derive(Debug, Default, PartialEq, Clone, Copy)]
        pub struct Health {
            pub r#state: ::core::option::Option<HealthState>,
            pub r#received_requests: ::core::option::Option<u32>,
            pub r#rejected_datagrams: ::core::option::Option<u32>,
            pub r#send_errors: ::core::option::Option<u32>,
            pub r#skipped_publications: ::core::option::Option<u32>,
        }
        impl Health {
            /// Return a reference to `state` as an `Option`
            #[inline]
            pub fn r#state(&self) -> ::core::option::Option<&HealthState> {
                self.r#state.as_ref()
            }
            /// Set the value and presence of `state`
            #[inline]
            pub fn set_state(&mut self, value: HealthState) -> &mut Self {
                self.r#state = ::core::option::Option::Some(value.into());
                self
            }
            /// Return a mutable reference to `state` as an `Option`
            #[inline]
            pub fn mut_state(&mut self) -> ::core::option::Option<&mut HealthState> {
                self.r#state.as_mut()
            }
            /// Clear the presence of `state`
            #[inline]
            pub fn clear_state(&mut self) -> &mut Self {
                self.r#state = ::core::option::Option::None;
                self
            }
            /// Take the value of `state` and clear its presence
            #[inline]
            pub fn take_state(&mut self) -> ::core::option::Option<HealthState> {
                self.r#state.take()
            }
            /// Builder method that sets the value of `state`. Useful for initializing the message.
            #[inline]
            pub fn init_state(mut self, value: HealthState) -> Self {
                self.set_state(value);
                self
            }
            /// Return a reference to `received_requests` as an `Option`
            #[inline]
            pub fn r#received_requests(&self) -> ::core::option::Option<&u32> {
                self.r#received_requests.as_ref()
            }
            /// Set the value and presence of `received_requests`
            #[inline]
            pub fn set_received_requests(&mut self, value: u32) -> &mut Self {
                self.r#received_requests = ::core::option::Option::Some(value.into());
                self
            }
            /// Return a mutable reference to `received_requests` as an `Option`
            #[inline]
            pub fn mut_received_requests(&mut self) -> ::core::option::Option<&mut u32> {
                self.r#received_requests.as_mut()
            }
            /// Clear the presence of `received_requests`
            #[inline]
            pub fn clear_received_requests(&mut self) -> &mut Self {
                self.r#received_requests = ::core::option::Option::None;
                self
            }
            /// Take the value of `received_requests` and clear its presence
            #[inline]
            pub fn take_received_requests(&mut self) -> ::core::option::Option<u32> {
                self.r#received_requests.take()
            }
            /// Builder method that sets the value of `received_requests`. Useful for initializing the message.
            #[inline]
            pub fn init_received_requests(mut self, value: u32) -> Self {
                self.set_received_requests(value);
                self
            }
            /// Return a reference to `rejected_datagrams` as an `Option`
            #[inline]
            pub fn r#rejected_datagrams(&self) -> ::core::option::Option<&u32> {
                self.r#rejected_datagrams.as_ref()
            }
            /// Set the value and presence of `rejected_datagrams`
            #[inline]
            pub fn set_rejected_datagrams(&mut self, value: u32) -> &mut Self {
                self.r#rejected_datagrams = ::core::option::Option::Some(value.into());
                self
            }
            /// Return a mutable reference to `rejected_datagrams` as an `Option`
            #[inline]
            pub fn mut_rejected_datagrams(
                &mut self,
            ) -> ::core::option::Option<&mut u32> {
                self.r#rejected_datagrams.as_mut()
            }
            /// Clear the presence of `rejected_datagrams`
            #[inline]
            pub fn clear_rejected_datagrams(&mut self) -> &mut Self {
                self.r#rejected_datagrams = ::core::option::Option::None;
                self
            }
            /// Take the value of `rejected_datagrams` and clear its presence
            #[inline]
            pub fn take_rejected_datagrams(&mut self) -> ::core::option::Option<u32> {
                self.r#rejected_datagrams.take()
            }
            /// Builder method that sets the value of `rejected_datagrams`. Useful for initializing the message.
            #[inline]
            pub fn init_rejected_datagrams(mut self, value: u32) -> Self {
                self.set_rejected_datagrams(value);
                self
            }
            /// Return a reference to `send_errors` as an `Option`
            #[inline]
            pub fn r#send_errors(&self) -> ::core::option::Option<&u32> {
                self.r#send_errors.as_ref()
            }
            /// Set the value and presence of `send_errors`
            #[inline]
            pub fn set_send_errors(&mut self, value: u32) -> &mut Self {
                self.r#send_errors = ::core::option::Option::Some(value.into());
                self
            }
            /// Return a mutable reference to `send_errors` as an `Option`
            #[inline]
            pub fn mut_send_errors(&mut self) -> ::core::option::Option<&mut u32> {
                self.r#send_errors.as_mut()
            }
            /// Clear the presence of `send_errors`
            #[inline]
            pub fn clear_send_errors(&mut self) -> &mut Self {
                self.r#send_errors = ::core::option::Option::None;
                self
            }
            /// Take the value of `send_errors` and clear its presence
            #[inline]
            pub fn take_send_errors(&mut self) -> ::core::option::Option<u32> {
                self.r#send_errors.take()
            }
            /// Builder method that sets the value of `send_errors`. Useful for initializing the message.
            #[inline]
            pub fn init_send_errors(mut self, value: u32) -> Self {
                self.set_send_errors(value);
                self
            }
            /// Return a reference to `skipped_publications` as an `Option`
            #[inline]
            pub fn r#skipped_publications(&self) -> ::core::option::Option<&u32> {
                self.r#skipped_publications.as_ref()
            }
            /// Set the value and presence of `skipped_publications`
            #[inline]
            pub fn set_skipped_publications(&mut self, value: u32) -> &mut Self {
                self.r#skipped_publications = ::core::option::Option::Some(value.into());
                self
            }
            /// Return a mutable reference to `skipped_publications` as an `Option`
            #[inline]
            pub fn mut_skipped_publications(
                &mut self,
            ) -> ::core::option::Option<&mut u32> {
                self.r#skipped_publications.as_mut()
            }
            /// Clear the presence of `skipped_publications`
            #[inline]
            pub fn clear_skipped_publications(&mut self) -> &mut Self {
                self.r#skipped_publications = ::core::option::Option::None;
                self
            }
            /// Take the value of `skipped_publications` and clear its presence
            #[inline]
            pub fn take_skipped_publications(&mut self) -> ::core::option::Option<u32> {
                self.r#skipped_publications.take()
            }
            /// Builder method that sets the value of `skipped_publications`. Useful for initializing the message.
            #[inline]
            pub fn init_skipped_publications(mut self, value: u32) -> Self {
                self.set_skipped_publications(value);
                self
            }
        }
        impl ::micropb::MessageDecode for Health {
            fn decode<IMPL_MICROPB_READ: ::micropb::PbRead>(
                &mut self,
                decoder: &mut ::micropb::PbDecoder<IMPL_MICROPB_READ>,
                len: usize,
            ) -> Result<(), ::micropb::DecodeError<IMPL_MICROPB_READ::Error>> {
                use ::micropb::{PbBytes, PbString, PbVec, PbMap, FieldDecode};
                let before = decoder.bytes_read();
                while decoder.bytes_read() - before < len {
                    let tag = decoder.decode_tag()?;
                    match tag.field_num() {
                        0 => return Err(::micropb::DecodeError::ZeroField),
                        1u32 => {
                            let mut_ref = &mut *self
                                .r#state
                                .get_or_insert_with(::core::default::Default::default);
                            {
                                let val = decoder
                                    .decode_int32()
                                    .map(|n| HealthState(n as _))?;
                                *mut_ref = val as _;
                            };
                        }
                        2u32 => {
                            let mut_ref = &mut *self
                                .r#received_requests
                                .get_or_insert_with(::core::default::Default::default);
                            {
                                let val = decoder.decode_varint32()?;
                                *mut_ref = val as _;
                            };
                        }
                        3u32 => {
                            let mut_ref = &mut *self
                                .r#rejected_datagrams
                                .get_or_insert_with(::core::default::Default::default);
                            {
                                let val = decoder.decode_varint32()?;
                                *mut_ref = val as _;
                            };
                        }
                        4u32 => {
                            let mut_ref = &mut *self
                                .r#send_errors
                                .get_or_insert_with(::core::default::Default::default);
                            {
                                let val = decoder.decode_varint32()?;
                                *mut_ref = val as _;
                            };
                        }
                        5u32 => {
                            let mut_ref = &mut *self
                                .r#skipped_publications
                                .get_or_insert_with(::core::default::Default::default);
                            {
                                let val = decoder.decode_varint32()?;
                                *mut_ref = val as _;
                            };
                        }
                        _ => {
                            decoder.skip_wire_value(tag.wire_type())?;
                        }
                    }
                }
                Ok(())
            }
        }
        impl ::micropb::MessageEncode for Health {
            const MAX_SIZE: ::core::result::Result<usize, &'static str> = 'msg: {
                let mut max_size = 0;
                match ::micropb::const_map!(
                    ::core::result::Result::Ok(HealthState::_MAX_SIZE), | size | size +
                    1usize
                ) {
                    ::core::result::Result::Ok(size) => {
                        max_size += size;
                    }
                    ::core::result::Result::Err(err) => {
                        break 'msg (::core::result::Result::<usize, _>::Err(err));
                    }
                }
                match ::micropb::const_map!(
                    ::core::result::Result::Ok(5usize), | size | size + 1usize
                ) {
                    ::core::result::Result::Ok(size) => {
                        max_size += size;
                    }
                    ::core::result::Result::Err(err) => {
                        break 'msg (::core::result::Result::<usize, _>::Err(err));
                    }
                }
                match ::micropb::const_map!(
                    ::core::result::Result::Ok(5usize), | size | size + 1usize
                ) {
                    ::core::result::Result::Ok(size) => {
                        max_size += size;
                    }
                    ::core::result::Result::Err(err) => {
                        break 'msg (::core::result::Result::<usize, _>::Err(err));
                    }
                }
                match ::micropb::const_map!(
                    ::core::result::Result::Ok(5usize), | size | size + 1usize
                ) {
                    ::core::result::Result::Ok(size) => {
                        max_size += size;
                    }
                    ::core::result::Result::Err(err) => {
                        break 'msg (::core::result::Result::<usize, _>::Err(err));
                    }
                }
                match ::micropb::const_map!(
                    ::core::result::Result::Ok(5usize), | size | size + 1usize
                ) {
                    ::core::result::Result::Ok(size) => {
                        max_size += size;
                    }
                    ::core::result::Result::Err(err) => {
                        break 'msg (::core::result::Result::<usize, _>::Err(err));
                    }
                }
                ::core::result::Result::Ok(max_size)
            };
            fn encode<IMPL_MICROPB_WRITE: ::micropb::PbWrite>(
                &self,
                encoder: &mut ::micropb::PbEncoder<IMPL_MICROPB_WRITE>,
            ) -> Result<(), IMPL_MICROPB_WRITE::Error> {
                use ::micropb::{PbMap, FieldEncode};
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#state() {
                        encoder.encode_varint32(8u32)?;
                        encoder.encode_int32(val_ref.0 as _)?;
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self
                        .r#received_requests()
                    {
                        encoder.encode_varint32(16u32)?;
                        encoder.encode_varint32(*val_ref as _)?;
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self
                        .r#rejected_datagrams()
                    {
                        encoder.encode_varint32(24u32)?;
                        encoder.encode_varint32(*val_ref as _)?;
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#send_errors() {
                        encoder.encode_varint32(32u32)?;
                        encoder.encode_varint32(*val_ref as _)?;
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self
                        .r#skipped_publications()
                    {
                        encoder.encode_varint32(40u32)?;
                        encoder.encode_varint32(*val_ref as _)?;
                    }
                }
                Ok(())
            }
            fn compute_size(&self) -> usize {
                use ::micropb::{PbMap, FieldEncode};
                let mut size = 0;
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#state() {
                        size += 1usize + ::micropb::size::sizeof_int32(val_ref.0 as _);
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self
                        .r#received_requests()
                    {
                        size += 1usize + ::micropb::size::sizeof_varint32(*val_ref as _);
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self
                        .r#rejected_datagrams()
                    {
                        size += 1usize + ::micropb::size::sizeof_varint32(*val_ref as _);
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#send_errors() {
                        size += 1usize + ::micropb::size::sizeof_varint32(*val_ref as _);
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self
                        .r#skipped_publications()
                    {
                        size += 1usize + ::micropb::size::sizeof_varint32(*val_ref as _);
                    }
                }
                size
            }
        }
        #[derive(Debug, Default, PartialEq, Clone, Copy)]
        pub struct Error {
            pub r#code: ::core::option::Option<ErrorCode>,
        }
        impl Error {
            /// Return a reference to `code` as an `Option`
            #[inline]
            pub fn r#code(&self) -> ::core::option::Option<&ErrorCode> {
                self.r#code.as_ref()
            }
            /// Set the value and presence of `code`
            #[inline]
            pub fn set_code(&mut self, value: ErrorCode) -> &mut Self {
                self.r#code = ::core::option::Option::Some(value.into());
                self
            }
            /// Return a mutable reference to `code` as an `Option`
            #[inline]
            pub fn mut_code(&mut self) -> ::core::option::Option<&mut ErrorCode> {
                self.r#code.as_mut()
            }
            /// Clear the presence of `code`
            #[inline]
            pub fn clear_code(&mut self) -> &mut Self {
                self.r#code = ::core::option::Option::None;
                self
            }
            /// Take the value of `code` and clear its presence
            #[inline]
            pub fn take_code(&mut self) -> ::core::option::Option<ErrorCode> {
                self.r#code.take()
            }
            /// Builder method that sets the value of `code`. Useful for initializing the message.
            #[inline]
            pub fn init_code(mut self, value: ErrorCode) -> Self {
                self.set_code(value);
                self
            }
        }
        impl ::micropb::MessageDecode for Error {
            fn decode<IMPL_MICROPB_READ: ::micropb::PbRead>(
                &mut self,
                decoder: &mut ::micropb::PbDecoder<IMPL_MICROPB_READ>,
                len: usize,
            ) -> Result<(), ::micropb::DecodeError<IMPL_MICROPB_READ::Error>> {
                use ::micropb::{PbBytes, PbString, PbVec, PbMap, FieldDecode};
                let before = decoder.bytes_read();
                while decoder.bytes_read() - before < len {
                    let tag = decoder.decode_tag()?;
                    match tag.field_num() {
                        0 => return Err(::micropb::DecodeError::ZeroField),
                        1u32 => {
                            let mut_ref = &mut *self
                                .r#code
                                .get_or_insert_with(::core::default::Default::default);
                            {
                                let val = decoder
                                    .decode_int32()
                                    .map(|n| ErrorCode(n as _))?;
                                *mut_ref = val as _;
                            };
                        }
                        _ => {
                            decoder.skip_wire_value(tag.wire_type())?;
                        }
                    }
                }
                Ok(())
            }
        }
        impl ::micropb::MessageEncode for Error {
            const MAX_SIZE: ::core::result::Result<usize, &'static str> = 'msg: {
                let mut max_size = 0;
                match ::micropb::const_map!(
                    ::core::result::Result::Ok(ErrorCode::_MAX_SIZE), | size | size +
                    1usize
                ) {
                    ::core::result::Result::Ok(size) => {
                        max_size += size;
                    }
                    ::core::result::Result::Err(err) => {
                        break 'msg (::core::result::Result::<usize, _>::Err(err));
                    }
                }
                ::core::result::Result::Ok(max_size)
            };
            fn encode<IMPL_MICROPB_WRITE: ::micropb::PbWrite>(
                &self,
                encoder: &mut ::micropb::PbEncoder<IMPL_MICROPB_WRITE>,
            ) -> Result<(), IMPL_MICROPB_WRITE::Error> {
                use ::micropb::{PbMap, FieldEncode};
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#code() {
                        encoder.encode_varint32(8u32)?;
                        encoder.encode_int32(val_ref.0 as _)?;
                    }
                }
                Ok(())
            }
            fn compute_size(&self) -> usize {
                use ::micropb::{PbMap, FieldEncode};
                let mut size = 0;
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#code() {
                        size += 1usize + ::micropb::size::sizeof_int32(val_ref.0 as _);
                    }
                }
                size
            }
        }
        #[derive(Debug, Default, PartialEq, Clone)]
        pub struct Envelope {
            pub r#protocol_version: ::core::option::Option<u32>,
            pub r#kind: ::core::option::Option<MessageKind>,
            pub r#device_id: ::core::option::Option<::heapless::String<32>>,
            pub r#boot_id: ::core::option::Option<::heapless::String<16>>,
            pub r#client_session: ::core::option::Option<::heapless::String<16>>,
            pub r#request_id: ::core::option::Option<u32>,
            pub r#sequence: ::core::option::Option<u32>,
            pub r#uptime_ms: ::core::option::Option<u32>,
            pub r#body: ::core::option::Option<Envelope_::Body>,
        }
        impl Envelope {
            /// Return a reference to `protocol_version` as an `Option`
            #[inline]
            pub fn r#protocol_version(&self) -> ::core::option::Option<&u32> {
                self.r#protocol_version.as_ref()
            }
            /// Set the value and presence of `protocol_version`
            #[inline]
            pub fn set_protocol_version(&mut self, value: u32) -> &mut Self {
                self.r#protocol_version = ::core::option::Option::Some(value.into());
                self
            }
            /// Return a mutable reference to `protocol_version` as an `Option`
            #[inline]
            pub fn mut_protocol_version(&mut self) -> ::core::option::Option<&mut u32> {
                self.r#protocol_version.as_mut()
            }
            /// Clear the presence of `protocol_version`
            #[inline]
            pub fn clear_protocol_version(&mut self) -> &mut Self {
                self.r#protocol_version = ::core::option::Option::None;
                self
            }
            /// Take the value of `protocol_version` and clear its presence
            #[inline]
            pub fn take_protocol_version(&mut self) -> ::core::option::Option<u32> {
                self.r#protocol_version.take()
            }
            /// Builder method that sets the value of `protocol_version`. Useful for initializing the message.
            #[inline]
            pub fn init_protocol_version(mut self, value: u32) -> Self {
                self.set_protocol_version(value);
                self
            }
            /// Return a reference to `kind` as an `Option`
            #[inline]
            pub fn r#kind(&self) -> ::core::option::Option<&MessageKind> {
                self.r#kind.as_ref()
            }
            /// Set the value and presence of `kind`
            #[inline]
            pub fn set_kind(&mut self, value: MessageKind) -> &mut Self {
                self.r#kind = ::core::option::Option::Some(value.into());
                self
            }
            /// Return a mutable reference to `kind` as an `Option`
            #[inline]
            pub fn mut_kind(&mut self) -> ::core::option::Option<&mut MessageKind> {
                self.r#kind.as_mut()
            }
            /// Clear the presence of `kind`
            #[inline]
            pub fn clear_kind(&mut self) -> &mut Self {
                self.r#kind = ::core::option::Option::None;
                self
            }
            /// Take the value of `kind` and clear its presence
            #[inline]
            pub fn take_kind(&mut self) -> ::core::option::Option<MessageKind> {
                self.r#kind.take()
            }
            /// Builder method that sets the value of `kind`. Useful for initializing the message.
            #[inline]
            pub fn init_kind(mut self, value: MessageKind) -> Self {
                self.set_kind(value);
                self
            }
            /// Return a reference to `device_id` as an `Option`
            #[inline]
            pub fn r#device_id(
                &self,
            ) -> ::core::option::Option<&::heapless::String<32>> {
                self.r#device_id.as_ref()
            }
            /// Set the value and presence of `device_id`
            #[inline]
            pub fn set_device_id(&mut self, value: ::heapless::String<32>) -> &mut Self {
                self.r#device_id = ::core::option::Option::Some(value.into());
                self
            }
            /// Return a mutable reference to `device_id` as an `Option`
            #[inline]
            pub fn mut_device_id(
                &mut self,
            ) -> ::core::option::Option<&mut ::heapless::String<32>> {
                self.r#device_id.as_mut()
            }
            /// Clear the presence of `device_id`
            #[inline]
            pub fn clear_device_id(&mut self) -> &mut Self {
                self.r#device_id = ::core::option::Option::None;
                self
            }
            /// Take the value of `device_id` and clear its presence
            #[inline]
            pub fn take_device_id(
                &mut self,
            ) -> ::core::option::Option<::heapless::String<32>> {
                self.r#device_id.take()
            }
            /// Builder method that sets the value of `device_id`. Useful for initializing the message.
            #[inline]
            pub fn init_device_id(mut self, value: ::heapless::String<32>) -> Self {
                self.set_device_id(value);
                self
            }
            /// Return a reference to `boot_id` as an `Option`
            #[inline]
            pub fn r#boot_id(&self) -> ::core::option::Option<&::heapless::String<16>> {
                self.r#boot_id.as_ref()
            }
            /// Set the value and presence of `boot_id`
            #[inline]
            pub fn set_boot_id(&mut self, value: ::heapless::String<16>) -> &mut Self {
                self.r#boot_id = ::core::option::Option::Some(value.into());
                self
            }
            /// Return a mutable reference to `boot_id` as an `Option`
            #[inline]
            pub fn mut_boot_id(
                &mut self,
            ) -> ::core::option::Option<&mut ::heapless::String<16>> {
                self.r#boot_id.as_mut()
            }
            /// Clear the presence of `boot_id`
            #[inline]
            pub fn clear_boot_id(&mut self) -> &mut Self {
                self.r#boot_id = ::core::option::Option::None;
                self
            }
            /// Take the value of `boot_id` and clear its presence
            #[inline]
            pub fn take_boot_id(
                &mut self,
            ) -> ::core::option::Option<::heapless::String<16>> {
                self.r#boot_id.take()
            }
            /// Builder method that sets the value of `boot_id`. Useful for initializing the message.
            #[inline]
            pub fn init_boot_id(mut self, value: ::heapless::String<16>) -> Self {
                self.set_boot_id(value);
                self
            }
            /// Return a reference to `client_session` as an `Option`
            #[inline]
            pub fn r#client_session(
                &self,
            ) -> ::core::option::Option<&::heapless::String<16>> {
                self.r#client_session.as_ref()
            }
            /// Set the value and presence of `client_session`
            #[inline]
            pub fn set_client_session(
                &mut self,
                value: ::heapless::String<16>,
            ) -> &mut Self {
                self.r#client_session = ::core::option::Option::Some(value.into());
                self
            }
            /// Return a mutable reference to `client_session` as an `Option`
            #[inline]
            pub fn mut_client_session(
                &mut self,
            ) -> ::core::option::Option<&mut ::heapless::String<16>> {
                self.r#client_session.as_mut()
            }
            /// Clear the presence of `client_session`
            #[inline]
            pub fn clear_client_session(&mut self) -> &mut Self {
                self.r#client_session = ::core::option::Option::None;
                self
            }
            /// Take the value of `client_session` and clear its presence
            #[inline]
            pub fn take_client_session(
                &mut self,
            ) -> ::core::option::Option<::heapless::String<16>> {
                self.r#client_session.take()
            }
            /// Builder method that sets the value of `client_session`. Useful for initializing the message.
            #[inline]
            pub fn init_client_session(mut self, value: ::heapless::String<16>) -> Self {
                self.set_client_session(value);
                self
            }
            /// Return a reference to `request_id` as an `Option`
            #[inline]
            pub fn r#request_id(&self) -> ::core::option::Option<&u32> {
                self.r#request_id.as_ref()
            }
            /// Set the value and presence of `request_id`
            #[inline]
            pub fn set_request_id(&mut self, value: u32) -> &mut Self {
                self.r#request_id = ::core::option::Option::Some(value.into());
                self
            }
            /// Return a mutable reference to `request_id` as an `Option`
            #[inline]
            pub fn mut_request_id(&mut self) -> ::core::option::Option<&mut u32> {
                self.r#request_id.as_mut()
            }
            /// Clear the presence of `request_id`
            #[inline]
            pub fn clear_request_id(&mut self) -> &mut Self {
                self.r#request_id = ::core::option::Option::None;
                self
            }
            /// Take the value of `request_id` and clear its presence
            #[inline]
            pub fn take_request_id(&mut self) -> ::core::option::Option<u32> {
                self.r#request_id.take()
            }
            /// Builder method that sets the value of `request_id`. Useful for initializing the message.
            #[inline]
            pub fn init_request_id(mut self, value: u32) -> Self {
                self.set_request_id(value);
                self
            }
            /// Return a reference to `sequence` as an `Option`
            #[inline]
            pub fn r#sequence(&self) -> ::core::option::Option<&u32> {
                self.r#sequence.as_ref()
            }
            /// Set the value and presence of `sequence`
            #[inline]
            pub fn set_sequence(&mut self, value: u32) -> &mut Self {
                self.r#sequence = ::core::option::Option::Some(value.into());
                self
            }
            /// Return a mutable reference to `sequence` as an `Option`
            #[inline]
            pub fn mut_sequence(&mut self) -> ::core::option::Option<&mut u32> {
                self.r#sequence.as_mut()
            }
            /// Clear the presence of `sequence`
            #[inline]
            pub fn clear_sequence(&mut self) -> &mut Self {
                self.r#sequence = ::core::option::Option::None;
                self
            }
            /// Take the value of `sequence` and clear its presence
            #[inline]
            pub fn take_sequence(&mut self) -> ::core::option::Option<u32> {
                self.r#sequence.take()
            }
            /// Builder method that sets the value of `sequence`. Useful for initializing the message.
            #[inline]
            pub fn init_sequence(mut self, value: u32) -> Self {
                self.set_sequence(value);
                self
            }
            /// Return a reference to `uptime_ms` as an `Option`
            #[inline]
            pub fn r#uptime_ms(&self) -> ::core::option::Option<&u32> {
                self.r#uptime_ms.as_ref()
            }
            /// Set the value and presence of `uptime_ms`
            #[inline]
            pub fn set_uptime_ms(&mut self, value: u32) -> &mut Self {
                self.r#uptime_ms = ::core::option::Option::Some(value.into());
                self
            }
            /// Return a mutable reference to `uptime_ms` as an `Option`
            #[inline]
            pub fn mut_uptime_ms(&mut self) -> ::core::option::Option<&mut u32> {
                self.r#uptime_ms.as_mut()
            }
            /// Clear the presence of `uptime_ms`
            #[inline]
            pub fn clear_uptime_ms(&mut self) -> &mut Self {
                self.r#uptime_ms = ::core::option::Option::None;
                self
            }
            /// Take the value of `uptime_ms` and clear its presence
            #[inline]
            pub fn take_uptime_ms(&mut self) -> ::core::option::Option<u32> {
                self.r#uptime_ms.take()
            }
            /// Builder method that sets the value of `uptime_ms`. Useful for initializing the message.
            #[inline]
            pub fn init_uptime_ms(mut self, value: u32) -> Self {
                self.set_uptime_ms(value);
                self
            }
        }
        impl ::micropb::MessageDecode for Envelope {
            fn decode<IMPL_MICROPB_READ: ::micropb::PbRead>(
                &mut self,
                decoder: &mut ::micropb::PbDecoder<IMPL_MICROPB_READ>,
                len: usize,
            ) -> Result<(), ::micropb::DecodeError<IMPL_MICROPB_READ::Error>> {
                use ::micropb::{PbBytes, PbString, PbVec, PbMap, FieldDecode};
                let before = decoder.bytes_read();
                while decoder.bytes_read() - before < len {
                    let tag = decoder.decode_tag()?;
                    match tag.field_num() {
                        0 => return Err(::micropb::DecodeError::ZeroField),
                        1u32 => {
                            let mut_ref = &mut *self
                                .r#protocol_version
                                .get_or_insert_with(::core::default::Default::default);
                            {
                                let val = decoder.decode_varint32()?;
                                *mut_ref = val as _;
                            };
                        }
                        2u32 => {
                            let mut_ref = &mut *self
                                .r#kind
                                .get_or_insert_with(::core::default::Default::default);
                            {
                                let val = decoder
                                    .decode_int32()
                                    .map(|n| MessageKind(n as _))?;
                                *mut_ref = val as _;
                            };
                        }
                        3u32 => {
                            let mut_ref = &mut *self
                                .r#device_id
                                .get_or_insert_with(::core::default::Default::default);
                            {
                                decoder
                                    .decode_string(mut_ref, ::micropb::Presence::Explicit)?;
                            };
                        }
                        4u32 => {
                            let mut_ref = &mut *self
                                .r#boot_id
                                .get_or_insert_with(::core::default::Default::default);
                            {
                                decoder
                                    .decode_string(mut_ref, ::micropb::Presence::Explicit)?;
                            };
                        }
                        5u32 => {
                            let mut_ref = &mut *self
                                .r#client_session
                                .get_or_insert_with(::core::default::Default::default);
                            {
                                decoder
                                    .decode_string(mut_ref, ::micropb::Presence::Explicit)?;
                            };
                        }
                        6u32 => {
                            let mut_ref = &mut *self
                                .r#request_id
                                .get_or_insert_with(::core::default::Default::default);
                            {
                                let val = decoder.decode_varint32()?;
                                *mut_ref = val as _;
                            };
                        }
                        7u32 => {
                            let mut_ref = &mut *self
                                .r#sequence
                                .get_or_insert_with(::core::default::Default::default);
                            {
                                let val = decoder.decode_varint32()?;
                                *mut_ref = val as _;
                            };
                        }
                        8u32 => {
                            let mut_ref = &mut *self
                                .r#uptime_ms
                                .get_or_insert_with(::core::default::Default::default);
                            {
                                let val = decoder.decode_varint32()?;
                                *mut_ref = val as _;
                            };
                        }
                        20u32 => {
                            let mut_ref = loop {
                                if let ::core::option::Option::Some(variant) = &mut self
                                    .r#body
                                {
                                    if let Envelope_::Body::GetDeviceInfo(variant) = &mut *variant {
                                        break &mut *variant;
                                    }
                                }
                                self.r#body = ::core::option::Option::Some(
                                    Envelope_::Body::GetDeviceInfo(
                                        ::core::default::Default::default(),
                                    ),
                                );
                            };
                            mut_ref.decode_len_delimited(decoder)?;
                        }
                        21u32 => {
                            let mut_ref = loop {
                                if let ::core::option::Option::Some(variant) = &mut self
                                    .r#body
                                {
                                    if let Envelope_::Body::GetStatus(variant) = &mut *variant {
                                        break &mut *variant;
                                    }
                                }
                                self.r#body = ::core::option::Option::Some(
                                    Envelope_::Body::GetStatus(
                                        ::core::default::Default::default(),
                                    ),
                                );
                            };
                            mut_ref.decode_len_delimited(decoder)?;
                        }
                        22u32 => {
                            let mut_ref = loop {
                                if let ::core::option::Option::Some(variant) = &mut self
                                    .r#body
                                {
                                    if let Envelope_::Body::GetHealth(variant) = &mut *variant {
                                        break &mut *variant;
                                    }
                                }
                                self.r#body = ::core::option::Option::Some(
                                    Envelope_::Body::GetHealth(
                                        ::core::default::Default::default(),
                                    ),
                                );
                            };
                            mut_ref.decode_len_delimited(decoder)?;
                        }
                        23u32 => {
                            let mut_ref = loop {
                                if let ::core::option::Option::Some(variant) = &mut self
                                    .r#body
                                {
                                    if let Envelope_::Body::DeviceInfo(variant) = &mut *variant {
                                        break &mut *variant;
                                    }
                                }
                                self.r#body = ::core::option::Option::Some(
                                    Envelope_::Body::DeviceInfo(
                                        ::core::default::Default::default(),
                                    ),
                                );
                            };
                            mut_ref.decode_len_delimited(decoder)?;
                        }
                        24u32 => {
                            let mut_ref = loop {
                                if let ::core::option::Option::Some(variant) = &mut self
                                    .r#body
                                {
                                    if let Envelope_::Body::Status(variant) = &mut *variant {
                                        break &mut *variant;
                                    }
                                }
                                self.r#body = ::core::option::Option::Some(
                                    Envelope_::Body::Status(::core::default::Default::default()),
                                );
                            };
                            mut_ref.decode_len_delimited(decoder)?;
                        }
                        25u32 => {
                            let mut_ref = loop {
                                if let ::core::option::Option::Some(variant) = &mut self
                                    .r#body
                                {
                                    if let Envelope_::Body::Health(variant) = &mut *variant {
                                        break &mut *variant;
                                    }
                                }
                                self.r#body = ::core::option::Option::Some(
                                    Envelope_::Body::Health(::core::default::Default::default()),
                                );
                            };
                            mut_ref.decode_len_delimited(decoder)?;
                        }
                        26u32 => {
                            let mut_ref = loop {
                                if let ::core::option::Option::Some(variant) = &mut self
                                    .r#body
                                {
                                    if let Envelope_::Body::Error(variant) = &mut *variant {
                                        break &mut *variant;
                                    }
                                }
                                self.r#body = ::core::option::Option::Some(
                                    Envelope_::Body::Error(::core::default::Default::default()),
                                );
                            };
                            mut_ref.decode_len_delimited(decoder)?;
                        }
                        _ => {
                            decoder.skip_wire_value(tag.wire_type())?;
                        }
                    }
                }
                Ok(())
            }
        }
        impl ::micropb::MessageEncode for Envelope {
            const MAX_SIZE: ::core::result::Result<usize, &'static str> = 'msg: {
                let mut max_size = 0;
                match ::micropb::const_map!(
                    ::core::result::Result::Ok(5usize), | size | size + 1usize
                ) {
                    ::core::result::Result::Ok(size) => {
                        max_size += size;
                    }
                    ::core::result::Result::Err(err) => {
                        break 'msg (::core::result::Result::<usize, _>::Err(err));
                    }
                }
                match ::micropb::const_map!(
                    ::core::result::Result::Ok(MessageKind::_MAX_SIZE), | size | size +
                    1usize
                ) {
                    ::core::result::Result::Ok(size) => {
                        max_size += size;
                    }
                    ::core::result::Result::Err(err) => {
                        break 'msg (::core::result::Result::<usize, _>::Err(err));
                    }
                }
                match ::micropb::const_map!(
                    ::core::result::Result::Ok(33usize), | size | size + 1usize
                ) {
                    ::core::result::Result::Ok(size) => {
                        max_size += size;
                    }
                    ::core::result::Result::Err(err) => {
                        break 'msg (::core::result::Result::<usize, _>::Err(err));
                    }
                }
                match ::micropb::const_map!(
                    ::core::result::Result::Ok(17usize), | size | size + 1usize
                ) {
                    ::core::result::Result::Ok(size) => {
                        max_size += size;
                    }
                    ::core::result::Result::Err(err) => {
                        break 'msg (::core::result::Result::<usize, _>::Err(err));
                    }
                }
                match ::micropb::const_map!(
                    ::core::result::Result::Ok(17usize), | size | size + 1usize
                ) {
                    ::core::result::Result::Ok(size) => {
                        max_size += size;
                    }
                    ::core::result::Result::Err(err) => {
                        break 'msg (::core::result::Result::<usize, _>::Err(err));
                    }
                }
                match ::micropb::const_map!(
                    ::core::result::Result::Ok(5usize), | size | size + 1usize
                ) {
                    ::core::result::Result::Ok(size) => {
                        max_size += size;
                    }
                    ::core::result::Result::Err(err) => {
                        break 'msg (::core::result::Result::<usize, _>::Err(err));
                    }
                }
                match ::micropb::const_map!(
                    ::core::result::Result::Ok(5usize), | size | size + 1usize
                ) {
                    ::core::result::Result::Ok(size) => {
                        max_size += size;
                    }
                    ::core::result::Result::Err(err) => {
                        break 'msg (::core::result::Result::<usize, _>::Err(err));
                    }
                }
                match ::micropb::const_map!(
                    ::core::result::Result::Ok(5usize), | size | size + 1usize
                ) {
                    ::core::result::Result::Ok(size) => {
                        max_size += size;
                    }
                    ::core::result::Result::Err(err) => {
                        break 'msg (::core::result::Result::<usize, _>::Err(err));
                    }
                }
                match 'oneof: {
                    let mut max_size = 0;
                    match ::micropb::const_map!(
                        ::micropb::const_map!(< Empty as ::micropb::MessageEncode >
                        ::MAX_SIZE, | size | ::micropb::size::sizeof_len_record(size)), |
                        size | size + 2usize
                    ) {
                        ::core::result::Result::Ok(size) => {
                            if size > max_size {
                                max_size = size;
                            }
                        }
                        ::core::result::Result::Err(err) => {
                            break 'oneof (::core::result::Result::<usize, _>::Err(err));
                        }
                    }
                    match ::micropb::const_map!(
                        ::micropb::const_map!(< Empty as ::micropb::MessageEncode >
                        ::MAX_SIZE, | size | ::micropb::size::sizeof_len_record(size)), |
                        size | size + 2usize
                    ) {
                        ::core::result::Result::Ok(size) => {
                            if size > max_size {
                                max_size = size;
                            }
                        }
                        ::core::result::Result::Err(err) => {
                            break 'oneof (::core::result::Result::<usize, _>::Err(err));
                        }
                    }
                    match ::micropb::const_map!(
                        ::micropb::const_map!(< Empty as ::micropb::MessageEncode >
                        ::MAX_SIZE, | size | ::micropb::size::sizeof_len_record(size)), |
                        size | size + 2usize
                    ) {
                        ::core::result::Result::Ok(size) => {
                            if size > max_size {
                                max_size = size;
                            }
                        }
                        ::core::result::Result::Err(err) => {
                            break 'oneof (::core::result::Result::<usize, _>::Err(err));
                        }
                    }
                    match ::micropb::const_map!(
                        ::micropb::const_map!(< DeviceInfo as ::micropb::MessageEncode >
                        ::MAX_SIZE, | size | ::micropb::size::sizeof_len_record(size)), |
                        size | size + 2usize
                    ) {
                        ::core::result::Result::Ok(size) => {
                            if size > max_size {
                                max_size = size;
                            }
                        }
                        ::core::result::Result::Err(err) => {
                            break 'oneof (::core::result::Result::<usize, _>::Err(err));
                        }
                    }
                    match ::micropb::const_map!(
                        ::micropb::const_map!(< Status as ::micropb::MessageEncode >
                        ::MAX_SIZE, | size | ::micropb::size::sizeof_len_record(size)), |
                        size | size + 2usize
                    ) {
                        ::core::result::Result::Ok(size) => {
                            if size > max_size {
                                max_size = size;
                            }
                        }
                        ::core::result::Result::Err(err) => {
                            break 'oneof (::core::result::Result::<usize, _>::Err(err));
                        }
                    }
                    match ::micropb::const_map!(
                        ::micropb::const_map!(< Health as ::micropb::MessageEncode >
                        ::MAX_SIZE, | size | ::micropb::size::sizeof_len_record(size)), |
                        size | size + 2usize
                    ) {
                        ::core::result::Result::Ok(size) => {
                            if size > max_size {
                                max_size = size;
                            }
                        }
                        ::core::result::Result::Err(err) => {
                            break 'oneof (::core::result::Result::<usize, _>::Err(err));
                        }
                    }
                    match ::micropb::const_map!(
                        ::micropb::const_map!(< Error as ::micropb::MessageEncode >
                        ::MAX_SIZE, | size | ::micropb::size::sizeof_len_record(size)), |
                        size | size + 2usize
                    ) {
                        ::core::result::Result::Ok(size) => {
                            if size > max_size {
                                max_size = size;
                            }
                        }
                        ::core::result::Result::Err(err) => {
                            break 'oneof (::core::result::Result::<usize, _>::Err(err));
                        }
                    }
                    ::core::result::Result::Ok(max_size)
                } {
                    ::core::result::Result::Ok(size) => {
                        max_size += size;
                    }
                    ::core::result::Result::Err(err) => {
                        break 'msg (::core::result::Result::<usize, _>::Err(err));
                    }
                }
                ::core::result::Result::Ok(max_size)
            };
            fn encode<IMPL_MICROPB_WRITE: ::micropb::PbWrite>(
                &self,
                encoder: &mut ::micropb::PbEncoder<IMPL_MICROPB_WRITE>,
            ) -> Result<(), IMPL_MICROPB_WRITE::Error> {
                use ::micropb::{PbMap, FieldEncode};
                {
                    if let ::core::option::Option::Some(val_ref) = self
                        .r#protocol_version()
                    {
                        encoder.encode_varint32(8u32)?;
                        encoder.encode_varint32(*val_ref as _)?;
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#kind() {
                        encoder.encode_varint32(16u32)?;
                        encoder.encode_int32(val_ref.0 as _)?;
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#device_id() {
                        encoder.encode_varint32(26u32)?;
                        encoder.encode_string(val_ref)?;
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#boot_id() {
                        encoder.encode_varint32(34u32)?;
                        encoder.encode_string(val_ref)?;
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self
                        .r#client_session()
                    {
                        encoder.encode_varint32(42u32)?;
                        encoder.encode_string(val_ref)?;
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#request_id() {
                        encoder.encode_varint32(48u32)?;
                        encoder.encode_varint32(*val_ref as _)?;
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#sequence() {
                        encoder.encode_varint32(56u32)?;
                        encoder.encode_varint32(*val_ref as _)?;
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#uptime_ms() {
                        encoder.encode_varint32(64u32)?;
                        encoder.encode_varint32(*val_ref as _)?;
                    }
                }
                if let Some(oneof) = &self.r#body {
                    match &*oneof {
                        Envelope_::Body::GetDeviceInfo(val_ref) => {
                            let val_ref = &*val_ref;
                            encoder.encode_varint32(162u32)?;
                            val_ref.encode_len_delimited(encoder)?;
                        }
                        Envelope_::Body::GetStatus(val_ref) => {
                            let val_ref = &*val_ref;
                            encoder.encode_varint32(170u32)?;
                            val_ref.encode_len_delimited(encoder)?;
                        }
                        Envelope_::Body::GetHealth(val_ref) => {
                            let val_ref = &*val_ref;
                            encoder.encode_varint32(178u32)?;
                            val_ref.encode_len_delimited(encoder)?;
                        }
                        Envelope_::Body::DeviceInfo(val_ref) => {
                            let val_ref = &*val_ref;
                            encoder.encode_varint32(186u32)?;
                            val_ref.encode_len_delimited(encoder)?;
                        }
                        Envelope_::Body::Status(val_ref) => {
                            let val_ref = &*val_ref;
                            encoder.encode_varint32(194u32)?;
                            val_ref.encode_len_delimited(encoder)?;
                        }
                        Envelope_::Body::Health(val_ref) => {
                            let val_ref = &*val_ref;
                            encoder.encode_varint32(202u32)?;
                            val_ref.encode_len_delimited(encoder)?;
                        }
                        Envelope_::Body::Error(val_ref) => {
                            let val_ref = &*val_ref;
                            encoder.encode_varint32(210u32)?;
                            val_ref.encode_len_delimited(encoder)?;
                        }
                    }
                }
                Ok(())
            }
            fn compute_size(&self) -> usize {
                use ::micropb::{PbMap, FieldEncode};
                let mut size = 0;
                {
                    if let ::core::option::Option::Some(val_ref) = self
                        .r#protocol_version()
                    {
                        size += 1usize + ::micropb::size::sizeof_varint32(*val_ref as _);
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#kind() {
                        size += 1usize + ::micropb::size::sizeof_int32(val_ref.0 as _);
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#device_id() {
                        size
                            += 1usize
                                + ::micropb::size::sizeof_len_record(val_ref.len());
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#boot_id() {
                        size
                            += 1usize
                                + ::micropb::size::sizeof_len_record(val_ref.len());
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self
                        .r#client_session()
                    {
                        size
                            += 1usize
                                + ::micropb::size::sizeof_len_record(val_ref.len());
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#request_id() {
                        size += 1usize + ::micropb::size::sizeof_varint32(*val_ref as _);
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#sequence() {
                        size += 1usize + ::micropb::size::sizeof_varint32(*val_ref as _);
                    }
                }
                {
                    if let ::core::option::Option::Some(val_ref) = self.r#uptime_ms() {
                        size += 1usize + ::micropb::size::sizeof_varint32(*val_ref as _);
                    }
                }
                if let Some(oneof) = &self.r#body {
                    match &*oneof {
                        Envelope_::Body::GetDeviceInfo(val_ref) => {
                            let val_ref = &*val_ref;
                            size
                                += 2usize
                                    + ::micropb::size::sizeof_len_record(
                                        val_ref.compute_size(),
                                    );
                        }
                        Envelope_::Body::GetStatus(val_ref) => {
                            let val_ref = &*val_ref;
                            size
                                += 2usize
                                    + ::micropb::size::sizeof_len_record(
                                        val_ref.compute_size(),
                                    );
                        }
                        Envelope_::Body::GetHealth(val_ref) => {
                            let val_ref = &*val_ref;
                            size
                                += 2usize
                                    + ::micropb::size::sizeof_len_record(
                                        val_ref.compute_size(),
                                    );
                        }
                        Envelope_::Body::DeviceInfo(val_ref) => {
                            let val_ref = &*val_ref;
                            size
                                += 2usize
                                    + ::micropb::size::sizeof_len_record(
                                        val_ref.compute_size(),
                                    );
                        }
                        Envelope_::Body::Status(val_ref) => {
                            let val_ref = &*val_ref;
                            size
                                += 2usize
                                    + ::micropb::size::sizeof_len_record(
                                        val_ref.compute_size(),
                                    );
                        }
                        Envelope_::Body::Health(val_ref) => {
                            let val_ref = &*val_ref;
                            size
                                += 2usize
                                    + ::micropb::size::sizeof_len_record(
                                        val_ref.compute_size(),
                                    );
                        }
                        Envelope_::Body::Error(val_ref) => {
                            let val_ref = &*val_ref;
                            size
                                += 2usize
                                    + ::micropb::size::sizeof_len_record(
                                        val_ref.compute_size(),
                                    );
                        }
                    }
                }
                size
            }
        }
        /// Inner types for `Envelope`
        pub mod Envelope_ {
            #[derive(Debug, PartialEq, Clone)]
            pub enum Body {
                GetDeviceInfo(super::Empty),
                GetStatus(super::Empty),
                GetHealth(super::Empty),
                DeviceInfo(super::DeviceInfo),
                Status(super::Status),
                Health(super::Health),
                Error(super::Error),
            }
        }
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(transparent)]
        pub struct MessageKind(pub i32);
        impl MessageKind {
            /// Maximum encoded size of the enum
            pub const _MAX_SIZE: usize = 10usize;
            pub const Unspecified: Self = Self(0);
            pub const GetDeviceInfo: Self = Self(1);
            pub const GetStatus: Self = Self(2);
            pub const GetHealth: Self = Self(3);
            pub const DeviceInfoResponse: Self = Self(11);
            pub const StatusResponse: Self = Self(12);
            pub const HealthResponse: Self = Self(13);
            pub const ErrorResponse: Self = Self(14);
            pub const StatusPublication: Self = Self(21);
            pub const HealthPublication: Self = Self(22);
        }
        impl core::default::Default for MessageKind {
            fn default() -> Self {
                Self(0)
            }
        }
        impl core::convert::From<i32> for MessageKind {
            fn from(val: i32) -> Self {
                Self(val)
            }
        }
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(transparent)]
        pub struct Encoding(pub i32);
        impl Encoding {
            /// Maximum encoded size of the enum
            pub const _MAX_SIZE: usize = 10usize;
            pub const Unspecified: Self = Self(0);
            pub const Csv: Self = Self(1);
            pub const Json: Self = Self(2);
            pub const Protobuf: Self = Self(3);
        }
        impl core::default::Default for Encoding {
            fn default() -> Self {
                Self(0)
            }
        }
        impl core::convert::From<i32> for Encoding {
            fn from(val: i32) -> Self {
                Self(val)
            }
        }
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(transparent)]
        pub struct Validity(pub i32);
        impl Validity {
            /// Maximum encoded size of the enum
            pub const _MAX_SIZE: usize = 10usize;
            pub const Unspecified: Self = Self(0);
            pub const Valid: Self = Self(1);
            pub const Unavailable: Self = Self(2);
            pub const SensorFault: Self = Self(3);
        }
        impl core::default::Default for Validity {
            fn default() -> Self {
                Self(0)
            }
        }
        impl core::convert::From<i32> for Validity {
            fn from(val: i32) -> Self {
                Self(val)
            }
        }
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(transparent)]
        pub struct HealthState(pub i32);
        impl HealthState {
            /// Maximum encoded size of the enum
            pub const _MAX_SIZE: usize = 10usize;
            pub const Unspecified: Self = Self(0);
            pub const Ok: Self = Self(1);
            pub const Degraded: Self = Self(2);
        }
        impl core::default::Default for HealthState {
            fn default() -> Self {
                Self(0)
            }
        }
        impl core::convert::From<i32> for HealthState {
            fn from(val: i32) -> Self {
                Self(val)
            }
        }
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(transparent)]
        pub struct ErrorCode(pub i32);
        impl ErrorCode {
            /// Maximum encoded size of the enum
            pub const _MAX_SIZE: usize = 10usize;
            pub const Unspecified: Self = Self(0);
            pub const UnsupportedKind: Self = Self(1);
            pub const InvalidRequest: Self = Self(2);
        }
        impl core::default::Default for ErrorCode {
            fn default() -> Self {
                Self(0)
            }
        }
        impl core::convert::From<i32> for ErrorCode {
            fn from(val: i32) -> Self {
                Self(val)
            }
        }
    }
}

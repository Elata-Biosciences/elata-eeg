//! Utility macros for the pipeline crate.

/// A declarative macro for defining a simple pipeline stage.
///
/// This macro generates a struct and a synchronous `Stage` implementation,
/// removing the need for most boilerplate. The user provides the stage name,
/// an optional block of state fields, and the core processing logic.
///
/// # Example
///
/// ```ignore
/// simple_stage!(
///     MyStage,
///     fields: {
///         my_state: u32 = 0,
///     },
///     process: {
///         // `self` refers to the stage struct instance.
///         self.my_state += 1;
///         Ok(Some(pkt))
///     }
/// );
/// ```
#[macro_export]
macro_rules! simple_stage {
    (
        $name:ident,
        fields: {
            $(
                $field_name:ident: $field_ty:ty = $field_init:expr
            ),* $(,)?
        },
        process: $process:block
    ) => {
        pub struct $name {
            id: String,
            $(
                $field_name: $field_ty,
            )*
        }

        impl $name {
            // The factory will call this
            pub fn new(id: String) -> Self {
                Self {
                    id,
                    $(
                        $field_name: $field_init,
                    )*
                }
            }
        }

        impl $crate::stage::Stage for $name {
            fn id(&self) -> &str {
                &self.id
            }

            fn process(
                &mut self,
                pkt: ::std::sync::Arc<$crate::data::RtPacket>,
                ctx: &mut $crate::stage::StageContext,
            ) -> Result<Option<::std::sync::Arc<$crate::data::RtPacket>>, $crate::error::StageError> {
                $process
            }
        }
    };
}

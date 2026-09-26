/// A model's explicit image-input declaration. Unknown support is not guessed
/// from a model name; an unsupported request can fail at the upstream API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageInputCapability {
    Supported,
    Unsupported,
    Unknown,
}

pub(crate) fn image_input_capability_from_modalities(
    _model: &str,
    modalities: Option<&[String]>,
) -> ImageInputCapability {
    match modalities {
        Some(items)
            if items
                .iter()
                .any(|item| item.trim().eq_ignore_ascii_case("image")) =>
        {
            ImageInputCapability::Supported
        }
        Some(_) => ImageInputCapability::Unsupported,
        None => ImageInputCapability::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_support_follows_the_explicit_model_declaration() {
        assert_eq!(
            image_input_capability_from_modalities(
                "gpt-6-luna",
                Some(&["text".into(), "image".into()])
            ),
            ImageInputCapability::Supported
        );
        assert_eq!(
            image_input_capability_from_modalities("gpt-6-astra", Some(&["text".into()])),
            ImageInputCapability::Unsupported
        );
        assert_eq!(
            image_input_capability_from_modalities("gpt-future", None),
            ImageInputCapability::Unknown
        );
    }
}

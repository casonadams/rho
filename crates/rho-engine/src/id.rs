/// Defines an id type wrapping a shared string, so ids of different kinds cannot be mixed up.
macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(std::sync::Arc<str>);

        impl $name {
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(id: &str) -> Self {
                Self(std::sync::Arc::from(id))
            }
        }

        impl From<String> for $name {
            fn from(id: String) -> Self {
                Self(std::sync::Arc::from(id))
            }
        }
    };
}

pub(crate) use string_id;

#[cfg(test)]
mod tests {
    string_id!(
        /// A test id.
        TestId
    );

    #[test]
    fn an_id_reads_and_prints_as_the_string_it_was_made_from() {
        let from_str = TestId::from("task-1");
        let from_string = TestId::from("task-1".to_string());
        assert_eq!(from_str, from_string);
        assert_eq!(from_str.as_str(), "task-1");
        assert_eq!(from_str.to_string(), "task-1");
    }
}

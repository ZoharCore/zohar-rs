pub(crate) mod content;

pub(crate) trait ToDomain<T> {
    fn to_domain(self) -> T;
}

#[allow(dead_code)]
pub(crate) trait ToProtocol<T> {
    fn to_protocol(self) -> T;
}

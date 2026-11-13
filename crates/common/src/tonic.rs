pub trait RewrapErr<T> {
    fn rewrap_err(self) -> Result<T, tonic::Status>;
}

impl<T> RewrapErr<T> for anyhow::Result<T> {
    fn rewrap_err(self) -> Result<T, tonic::Status> {
        self.map_err(|err| tonic::Status::internal(err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::RewrapErr;

    #[test]
    fn rewraps_anyhow_error_as_internal_status() {
        let status = Err::<(), _>(anyhow::anyhow!("failed operation"))
            .rewrap_err()
            .unwrap_err();

        assert_eq!(status.code(), tonic::Code::Internal);
        assert_eq!(status.message(), "failed operation");
    }
}

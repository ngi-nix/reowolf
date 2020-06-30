use super::*;

impl FileLogger {
    pub fn new(connector_id: ConnectorId, file: std::fs::File) -> Self {
        Self(connector_id, file)
    }
}
impl VecLogger {
    pub fn new(connector_id: ConnectorId) -> Self {
        Self(connector_id, Default::default())
    }
}
/////////////////
impl Logger for DummyLogger {
    fn line_writer(&mut self) -> &mut dyn std::io::Write {
        self
    }
}
impl Logger for VecLogger {
    fn line_writer(&mut self) -> &mut dyn std::io::Write {
        let _ = write!(&mut self.1, "CID({}) at {:?} ", self.0, Instant::now());
        self
    }
}
impl Logger for FileLogger {
    fn line_writer(&mut self) -> &mut dyn std::io::Write {
        let _ = write!(&mut self.1, "CID({}) at {:?} ", self.0, Instant::now());
        &mut self.1
    }
}
///////////////////
impl Drop for VecLogger {
    fn drop(&mut self) {
        let stdout = std::io::stderr();
        let mut lock = stdout.lock();
        writeln!(lock, "--- DROP LOG DUMP ---").unwrap();
        let _ = std::io::Write::write(&mut lock, self.1.as_slice());
    }
}
impl std::io::Write for VecLogger {
    fn flush(&mut self) -> Result<(), std::io::Error> {
        Ok(())
    }
    fn write(&mut self, data: &[u8]) -> Result<usize, std::io::Error> {
        self.1.extend_from_slice(data);
        Ok(data.len())
    }
}
impl std::io::Write for DummyLogger {
    fn flush(&mut self) -> Result<(), std::io::Error> {
        Ok(())
    }
    fn write(&mut self, bytes: &[u8]) -> Result<usize, std::io::Error> {
        Ok(bytes.len())
    }
}

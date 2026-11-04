use voice_abi::host::{BackendInstance, LoadError};

use crate::Suppressor;

impl Suppressor for BackendInstance {
    type Error = LoadError;

    fn process(&mut self, input: &[f32], output: &mut [f32]) -> Result<(), LoadError> {
        BackendInstance::process(self, input, output)
    }
}

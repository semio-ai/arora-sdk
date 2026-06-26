use std::borrow::Cow;

use uuid::Uuid;
use wasmtime::{AsContext, AsContextMut, Memory};

pub struct AroraBuffer<'a>(pub Cow<'a, [u8]>);

impl AsRef<[u8]> for AroraBuffer<'_> {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

pub trait ReadWasmMemory {
    fn read_wasm_memory<T, C: AsContext<Data = T>>(
        context: &C,
        memory: Memory,
        offset: u32,
    ) -> Self;
}

#[allow(dead_code)]
pub trait WriteWasmMemory {
    fn write_wasm_memory<T, C: AsContextMut<Data = T>>(
        &self,
        context: &mut C,
        memory: Memory,
        offset: u32,
    );
}

impl ReadWasmMemory for Uuid {
    fn read_wasm_memory<T, C: AsContext<Data = T>>(
        context: &C,
        memory: Memory,
        offset: u32,
    ) -> Self {
        let mut buffer = [0u8; 16];
        memory.read(context, offset as usize, &mut buffer).unwrap();
        Uuid::from_slice(&buffer).unwrap()
    }
}

impl WriteWasmMemory for Uuid {
    fn write_wasm_memory<T, C: AsContextMut<Data = T>>(
        &self,
        context: &mut C,
        memory: Memory,
        offset: u32,
    ) {
        memory
            .write(context, offset as usize, self.as_bytes())
            .unwrap();
    }
}

impl<const N: usize> ReadWasmMemory for [u8; N] {
    fn read_wasm_memory<T, C: AsContext<Data = T>>(
        context: &C,
        memory: Memory,
        offset: u32,
    ) -> Self {
        let mut buffer = [0u8; N];
        memory.read(context, offset as usize, &mut buffer).unwrap();
        buffer
    }
}

impl<const N: usize> WriteWasmMemory for [u8; N] {
    fn write_wasm_memory<T, C: AsContextMut<Data = T>>(
        &self,
        context: &mut C,
        memory: Memory,
        offset: u32,
    ) {
        memory.write(context, offset as usize, self).unwrap();
    }
}

impl ReadWasmMemory for u32 {
    fn read_wasm_memory<T, C: AsContext<Data = T>>(
        context: &C,
        memory: Memory,
        offset: u32,
    ) -> Self {
        let mut buffer = [0u8; 4];
        memory.read(context, offset as usize, &mut buffer).unwrap();
        u32::from_le_bytes(buffer)
    }
}

impl WriteWasmMemory for u32 {
    fn write_wasm_memory<T, C: AsContextMut<Data = T>>(
        &self,
        context: &mut C,
        memory: Memory,
        offset: u32,
    ) {
        self.to_le_bytes()
            .write_wasm_memory(context, memory, offset);
    }
}

impl<'a> ReadWasmMemory for AroraBuffer<'a> {
    fn read_wasm_memory<T, C: AsContext<Data = T>>(
        context: &C,
        memory: Memory,
        offset: u32,
    ) -> Self {
        let size = u32::read_wasm_memory(context, memory, offset);

        let mut buffer = vec![0; size as usize + 4];
        memory.read(context, offset as usize, &mut buffer).unwrap();

        AroraBuffer(Cow::Owned(buffer))
    }
}

impl<'a> WriteWasmMemory for AroraBuffer<'a> {
    fn write_wasm_memory<T, C: AsContextMut<Data = T>>(
        &self,
        context: &mut C,
        memory: Memory,
        offset: u32,
    ) {
        memory
            .write(context, offset as usize, self.as_ref())
            .unwrap();
    }
}

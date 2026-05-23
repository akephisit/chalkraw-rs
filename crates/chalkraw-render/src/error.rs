// wgpu 29 API note: wgpu::SurfaceError does not exist in 29.x; surface texture
// acquisition returns CurrentSurfaceTexture (an enum), not a Result. The nearest
// error type for surface creation is wgpu::CreateSurfaceError.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("no GPU adapter available")]
    NoAdapter,

    #[error("device request failed: {0}")]
    DeviceRequest(#[from] wgpu::RequestDeviceError),

    #[error("surface error: {0}")]
    Surface(#[from] wgpu::CreateSurfaceError),

    #[error("buffer map error: {0}")]
    BufferMap(#[from] wgpu::BufferAsyncError),
}

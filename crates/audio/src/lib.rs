//! OS audio capture adapters implementing the application `AudioSource` port.
//!
//! Real-time callbacks only write to bounded ring buffers; resampling and any heavy work run
//! outside the callback (Constitution Art. V.3).

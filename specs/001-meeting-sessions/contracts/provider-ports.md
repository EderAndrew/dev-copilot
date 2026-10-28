# Contract: Portas da camada `application`

Esboço das portas (traits) que os crates de infraestrutura implementam e `apps/desktop/src-tauri` compõe
(Art. II.3). Assinaturas ilustrativas; nomes finais podem mudar na implementação sem alterar a semântica.
Tipos de fornecedor nunca atravessam estas portas (Art. VI.2).

```rust
/// Fonte de áudio de uma plataforma. Implementações: `audio::cpal_mic`, `audio::{macos,windows,linux}_system`.
pub trait AudioSource: Send {
    fn kind(&self) -> AudioSourceKind;
    fn availability(&self) -> Availability;
    /// Consulta o SO; não dispara prompt.
    fn permission(&self) -> PermissionState;
    /// Pode disparar o prompt do SO.
    async fn request_permission(&self) -> PermissionState;
    /// Inicia captura escrevendo PCM 16 kHz mono f32 em `sink` (limitado).
    /// Garantias: retorna erro sem captar se a permissão não estiver concedida;
    /// para de captar imediatamente quando `cancel` é acionado; nunca troca de dispositivo silenciosamente.
    fn start(&mut self, sink: BoundedAudioSink, cancel: CancellationToken) -> Result<CaptureHandle, AudioError>;
}

/// Transcrição em streaming. Implementações: `intelligence::whisper_local`, `intelligence::openai_compatible_stt`.
pub trait Transcriber: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor; // id, locality, data categories
    /// Consome chunks e emite parciais/finais. Respeita `cancel` em até 5 s.
    async fn run(&self, input: AudioChunkStream, out: TranscriptSink, cancel: CancellationToken)
        -> Result<(), ProviderError>;
}

/// Análise técnica e perguntas. Implementações: `intelligence::{anthropic, openai_compatible_chat, ollama}`.
pub trait AnalysisProvider: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    /// Retorna saída já validada contra `analysis-output.schema.json`; saída inválida → `ProviderError::InvalidOutput`.
    async fn analyze(&self, req: AnalysisRequest, cancel: CancellationToken) -> Result<AnalysisOutput, ProviderError>;
    async fn answer(&self, req: QuestionRequest, cancel: CancellationToken) -> Result<AnswerOutput, ProviderError>;
}

/// Ponto único de enforcement de rede para adaptadores remotos (R9).
pub trait RemoteGate: Send + Sync {
    /// Ok somente se existe autorização `granted` cobrindo destino e categorias.
    fn check(&self, provider: &ProviderId, categories: &[DataCategory]) -> Result<(), AuthorizationError>;
}

pub trait SessionRepository { /* create, get, update_state, list, delete_cascade, mark_interrupted_on_startup */ }
pub trait SegmentRepository { /* append_final, append_gap, list_by_session */ }
pub trait RecordRepository  { /* insert_proposed, apply_user_change (com RecordChange), list_by_session */ }
pub trait QuestionRepository { /* insert, set_answer, list_by_session */ }
pub trait ProviderRepository { /* providers + authorizations */ }
pub trait SettingsRepository { /* get/set chave-valor não sensível */ }
pub trait SecretStore { /* check_available, get/set/delete por CredentialRef; erro distinto para cofre indisponível */ }
pub trait Clock { fn now(&self) -> Timestamp; }
pub trait EventPublisher { fn publish(&self, session: SessionId, event: SessionEvent); }
```

## Regras de segurança aplicáveis a todos os provedores de IA (Art. IV.6, IV.14)

1. O conteúdo da sessão é enviado como **dados delimitados**, nunca concatenado às instruções do sistema.
2. Nenhuma ferramenta, função ou capacidade de ação é oferecida ao modelo.
3. A saída é aceita apenas como JSON válido segundo o schema; campos desconhecidos são rejeitados.
4. Ids de evidência retornados são verificados contra a sessão; ids inexistentes são descartados.
5. A saída só pode **criar propostas** (`status = proposed`) — nunca alterar ou excluir.
6. Antes de qualquer chamada remota, `RemoteGate::check` é obrigatório (testado com servidor fake).

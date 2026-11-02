# Contract: política de conteúdo de produção e destinos de rede

## CSP de produção (`app.security.csp` em `apps/desktop/src-tauri/tauri.conf.json`)

```text
default-src 'self';
script-src 'self';
style-src 'self';
img-src 'self' asset: http://asset.localhost data:;
font-src 'self';
connect-src 'self' ipc: http://ipc.localhost;
object-src 'none';
base-uri 'self';
form-action 'none'
```

A única mudança em relação à 001 é a remoção de `'unsafe-inline'` de `style-src`. As outras diretivas ficam
iguais. Hashes que o próprio Tauri acrescenta para assets empacotados são permitidos (R1).

**Invariantes verificados por `apps/desktop/src-tauri/tests/csp_policy.rs`**:

1. Nenhuma diretiva contém `'unsafe-inline'`, `'unsafe-eval'`, `'unsafe-hashes'`, `'wasm-unsafe-eval'`, `*`,
   `http:` ou `https:` sem host.
2. `connect-src` contém só `'self'`, `ipc:` e `http://ipc.localhost`. As requisições de atualização saem do
   núcleo, não da webview, e **não** entram aqui.
3. `object-src 'none'`, `base-uri 'self'` e `form-action 'none'` estão presentes.
4. Um hash `'sha256-…'` em `style-src` só é aceito se constar na lista de exceções do próprio teste, com
   referência a um ADR. A lista começa vazia.

A CSP de desenvolvimento (`devCsp`) pode continuar como está (FR-003). Ela nunca é usada para validar esta
feature.

## Destinos de rede do núcleo introduzidos por esta feature

| Destino | Quando | O que vai | Quem decide |
|---------|--------|-----------|-------------|
| `https://github.com/EderAndrew/dev-copilot/releases/latest/download/latest.json` | Verificação de atualização habilitada e aviso dispensado: na inicialização, a cada 24 h, ou manual | GET sem corpo; cabeçalhos HTTP padrão (user-agent com versão do app) | `UpdateService` |
| `https://objects.githubusercontent.com/…` (e o host de redirecionamento de assets do GitHub em uso) | Seguindo o redirecionamento do manifesto e download do pacote confirmado | GET sem corpo | `UpdateService` |

Nenhum outro destino novo. Os destinos da 001 (download de modelos, provedores autorizados) não mudam. O
aviso de primeira execução e a documentação ao usuário citam esses destinos (FR-008b).

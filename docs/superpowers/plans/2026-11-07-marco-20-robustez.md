# Marco 20 — robustez e recuperação Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transformar o kernel em uma versão que registre falhas, imponha limites, sobreviva a corrupção e interrupções durante operações do RPFS e demonstre isso em testes de carga e reinicialização.

**Architecture:** As três abordagens serão implementadas em sequência, cada uma deixando uma melhoria testável: (1) observabilidade, limites e carga; (2) validação e recuperação incremental do formato RPFS existente; (3) formato RPFS v2 com superblocos duplos e journal de transações. O RPFS v1 será montado em modo compatível e migrado para v2 sem apagar arquivos válidos.

**Tech Stack:** Rust `no_std`, AArch64 bare-metal, SDHOST BCM2837, RPFS, miniUART, QEMU `raspi3b`, scripts Bash/Python existentes.

## Global Constraints

- O kernel não usará `alloc`, threads de host ou dependências externas.
- Todo estado temporário terá tamanho fixo e limites explícitos.
- Nenhuma falha de cartão, entrada ou corrupção poderá sobrescrever MMIO, pilha ou regiões fora do RPFS.
- A UART continuará disponível mesmo quando o RPFS estiver indisponível.
- O formato antigo do RPFS será reconhecido; a migração só substituirá dados após validação completa.
- Cada alteração de código atualizará `agent.md` e `README.md`, rodará `make check` e, se mudar a imagem, executará `make write DEST=/media/vladimir/PENDRIVE` após confirmar a mídia.
- O Marco 20 só será declarado concluído após teste automatizado, QEMU e teste físico no Raspberry Pi 3B+.

---

## Estratégias que serão incorporadas

### Abordagem 1 — observabilidade, limites e carga

Adicionar logs estruturados de boot, SD, montagem, recuperação e falhas; validar tamanhos, índices, nomes, buffers e tempos; e criar uma sequência de estresse repetível no QEMU. Essa camada deve funcionar mesmo quando o RPFS não puder ser montado.

### Abordagem 2 — endurecimento incremental do RPFS v1

Manter o layout atual (`SB=64`, raiz no bloco `65`, bitmap no `66` e dados a partir do `67`), mas validar o superbloco, bitmap, diretórios e referências antes de usar qualquer bloco. Operações parcialmente escritas serão detectadas por checksums e marcadores de estado, com falha fechada e restauração do último estado válido.

### Abordagem 3 — RPFS v2 com redundância e journal

Introduzir dois superblocos alternados e um journal fixo dentro da região RPFS. Uma transação será preparada, gravada, confirmada e somente então publicada no superbloco ativo. No boot, o kernel escolherá a cópia válida mais recente, terminará transações confirmadas e descartará transações incompletas.

---

## Task 1: Contrato de erro, logs e limites

**Files:**
- Create: `src/log.rs`
- Create: `src/limits.rs`
- Modify: `src/main.rs`
- Modify: `src/console.rs`

**Interfaces:**
- `log::info(event: &'static str)`, `log::warn(event: &'static str)` e `log::error(event: &'static str)` escrevem mensagens limitadas na UART.
- `log::snapshot(dest: &mut [[u8; 32]; 8]) -> usize` copia o anel fixo de eventos sem alocar.
- `limits::valid_block(block: u32) -> bool`, `limits::valid_name(name: &str) -> bool` e `limits::valid_file_size(size: usize) -> bool` centralizam os limites do RPFS.
- O comando UART `logs` imprime os últimos eventos e `limites` imprime os limites ativos.

- [ ] **Step 1: Definir os limites verificáveis**

Usar os limites físicos existentes: blocos RPFS `64..=82`, nomes de no máximo 15 bytes, arquivos de no máximo 512 bytes, oito eventos de no máximo 31 bytes e timeout de I/O de no máximo 500 ms em unidades do `CLO`.

- [ ] **Step 2: Implementar o anel fixo de logs**

Criar `LogState` com oito entradas, comprimento por entrada e cursor circular. A escrita deve truncar eventos longos, nunca bloquear esperando o cartão e sempre tentar preservar a UART.

- [ ] **Step 3: Instrumentar boot, SD e montagem**

Registrar eventos `boot`, `sd-open`, `sd-read-fail`, `sd-write-fail`, `rpfs-mount`, `rpfs-invalid` e `rpfs-recovered`. Os eventos não devem substituir as respostas atuais dos comandos.

- [ ] **Step 4: Expor comandos de diagnóstico**

Adicionar `logs` e `limites` ao `help`. Os comandos devem continuar funcionando quando `sd::open()` ou `fs::mount()` falharem.

- [ ] **Step 5: Validar compilação e limites**

Executar `cargo build --release`, `bash -n scripts/check-qemu.sh` e `make check`. O teste deve verificar que nomes, blocos e tamanhos inválidos são rejeitados sem panic.

---

## Task 2: Falhas controladas e teste de carga

**Files:**
- Modify: `src/sd.rs`
- Modify: `src/fs.rs`
- Modify: `src/console.rs`
- Modify: `scripts/check-qemu.sh`

**Interfaces:**
- `sd::set_fault_mode(mode: sd::FaultMode)` será usado apenas pelo comando de diagnóstico.
- `sd::FaultMode` terá `None`, `FailNextRead`, `FailNextWrite` e `FailAfter(u32)`.
- `fs::check() -> FsReport` validará o RPFS sem modificar o cartão.
- Os comandos `falha leitura`, `falha escrita`, `fsck` e `stress` serão determinísticos.

- [ ] **Step 1: Adicionar injeção de falhas**

Aplicar a falha antes da transação real de leitura/escrita, registrar o evento e devolver `false`. O modo deve ser consumido uma única vez, exceto `FailAfter`, que decrementa a contagem.

- [ ] **Step 2: Implementar `fs::check()`**

Validar marca, versão, blocos dentro da região, entradas UTF-8, nomes terminados, tipos válidos, tamanhos até 512, referências sem duplicidade e bitmap coerente. O relatório deve distinguir `ok`, `corrompido` e `incompleto`.

- [ ] **Step 3: Implementar o comando `stress`**

Executar repetidamente criação/leitura de arquivos temporários dentro do limite de 16 blocos, conferir cada conteúdo e remover ou reutilizar os blocos sem deixar referências órfãs. O comando deve terminar com `stress: ok` ou `stress: falhou`.

- [ ] **Step 4: Estender o teste automatizado**

No `check-qemu.sh`, executar `limites`, `fsck`, `stress`, uma falha de leitura e uma falha de escrita. Confirmar que o prompt volta, os eventos aparecem em `logs` e não há `PANIC`.

- [ ] **Step 5: Rodar a carga**

Executar `make check` e confirmar que 100 ciclos alternados de leitura da raiz e de subdiretório terminam sem corrupção.

---

## Task 3: Checksums e recuperação do RPFS v1

**Files:**
- Modify: `src/fs.rs`
- Modify: `src/sd.rs`
- Modify: `scripts/check-qemu.sh`

**Interfaces:**
- `fs::mount() -> MountStatus` substituirá leituras implícitas de `mounted()`.
- `fs::recover() -> RecoveryStatus` validará e restaurará o último estado consistente.
- `fs::checksum(block: &[u8; 512]) -> u32` usará uma função determinística sem tabela dinâmica.

- [ ] **Step 1: Definir os metadados de integridade**

Reservar campos ainda não usados do superbloco v1 para checksum do superbloco, checksum da raiz e geração. Para blocos de diretório, usar os quatro bytes finais como checksum e reduzir a área de entradas somente nessa versão validada.

- [ ] **Step 2: Validar antes de publicar alterações**

Toda criação deve preparar o bloco de dados, preparar o diretório e conferir as leituras; somente depois atualizar o bitmap. Em falha, o bitmap deve ser restaurado ao valor anterior.

- [ ] **Step 3: Implementar recuperação**

Durante a montagem, se o checksum do diretório ou bitmap falhar, reconstruir o bitmap a partir das referências alcançáveis. Se houver duas referências para o mesmo bloco ou bloco fora da região, montar em modo somente leitura e reportar `rpfs: corrompido`.

- [ ] **Step 4: Testar corrupção determinística**

Adicionar ao script Python a alteração de um byte no bitmap, na raiz e em um bloco de dados entre dois boots. Confirmar `fsck: recuperado` quando possível, `fsck: corrompido` quando ambíguo e ausência de escrita fora da região.

- [ ] **Step 5: Preservar compatibilidade**

Testar uma imagem RPFS v1 criada antes da mudança, montar, listar e ler os arquivos; somente depois permitir a atualização dos metadados.

---

## Task 4: RPFS v2 com superblocos duplos

**Files:**
- Modify: `src/fs.rs`
- Modify: `src/console.rs`
- Modify: `scripts/check-qemu.sh`

**Interfaces:**
- `FsVersion::{V1, V2}` identifica o layout montado.
- `SuperblockV2` conterá marca, versão, geração, raiz, bitmap, journal, número de blocos e checksum.
- `fs::format_v2() -> bool` criará duas cópias válidas do superbloco.
- `fs::migrate_v1() -> bool` copiará apenas estruturas previamente validadas.

- [ ] **Step 1: Fixar o layout v2**

Usar os blocos 64 e 65 para superblocos A/B, bloco 66 para o journal, bloco 67 para bitmap, bloco 68 para raiz e blocos 69..82 para dados. Cada superbloco terá geração e checksum; o maior número válido será o ativo.

- [ ] **Step 2: Implementar seleção do superbloco**

Ler as duas cópias, validar marca, versão, limites, checksum e geração. Se somente uma for válida, montar usando-a e registrar `rpfs: superbloco alternativo`.

- [ ] **Step 3: Implementar migração**

Ao encontrar RPFS v1 válido, copiar entradas e dados para v2, verificar cada cópia e só então gravar os dois superblocos v2. Se a migração falhar, manter v1 intacto.

- [ ] **Step 4: Atualizar operações de arquivo**

Fazer `create_file`, `create_dir`, `alloc` e atualizações de diretório usarem os endereços do superbloco ativo, sem constantes de layout espalhadas pelo módulo.

- [ ] **Step 5: Testar reinicialização e cópia alternativa**

Criar dados, reiniciar, corromper uma cópia do superbloco e reiniciar novamente. Confirmar montagem pela cópia restante, conteúdo preservado e geração incrementada.

---

## Task 5: Journal transacional e recuperação após interrupção

**Files:**
- Modify: `src/fs.rs`
- Modify: `src/sd.rs`
- Modify: `scripts/check-qemu.sh`

**Interfaces:**
- `JournalRecord` conterá estado (`Prepared`, `Committed`), operação, geração, blocos afetados, dados antigos, dados novos e checksum.
- `fs::begin_transaction(op: FsOp) -> Option<TxId>`.
- `fs::commit_transaction(id: TxId) -> bool`.
- `fs::recover_journal() -> RecoveryStatus`.

- [ ] **Step 1: Implementar registro preparado**

Gravar o registro completo no bloco de journal, verificar por leitura e só então marcar `Prepared`. O journal terá tamanho fixo e rejeitará operações que não caibam.

- [ ] **Step 2: Implementar commit ordenado**

Aplicar os blocos novos, verificar cada um, marcar `Committed` e publicar o superbloco alternativo com geração maior. Nunca alterar o superbloco ativo antes do commit.

- [ ] **Step 3: Implementar recuperação no boot**

Ao montar, `Prepared` será descartado e o superbloco anterior permanecerá ativo; `Committed` será reaplicado ou finalizado idempotentemente. Depois, limpar o journal e registrar o resultado.

- [ ] **Step 4: Injetar interrupção em pontos definidos**

Adicionar modos de parada após `Prepared`, após gravação de dados e após `Committed`. O QEMU será encerrado pelo script e reiniciado com a mesma imagem.

- [ ] **Step 5: Validar invariantes**

Para cada ponto de interrupção, confirmar que não existem blocos duplicados, bitmap incoerente, arquivo parcialmente visível ou perda de arquivo já confirmado.

---

## Task 6: Carga prolongada e regressão completa

**Files:**
- Modify: `scripts/check-qemu.sh`
- Modify: `README.md`
- Modify: `agent.md`

- [ ] **Step 1: Criar a sequência de carga**

Executar pelo menos 100 operações alternadas de `ls`, `cat`, criação, leitura, `fsck`, `logs`, `stress` e reinício do QEMU, sempre comparando as respostas esperadas.

- [ ] **Step 2: Cobrir limites e erros**

Testar nome vazio, nome com mais de 15 bytes, arquivo de 513 bytes, bloco fora da região, bitmap cheio, journal inválido, superbloco inválido e falhas consecutivas de SD.

- [ ] **Step 3: Preservar regressões**

Manter os testes dos marcos 1–19: boot, UART, exceções, timer, MMU, EL0, USB/GUI, `tela`, RPFS, `run`, `ls`, `cat` e shell.

- [ ] **Step 4: Atualizar documentação**

Documentar layout v2, recuperação, comandos `logs`, `limites`, `fsck` e `stress`, procedimento de corrupção controlada e limitações do hardware.

- [ ] **Step 5: Rodar verificação automatizada**

Executar `make check`, `cargo build --release`, `bash -n scripts/check-qemu.sh` e `git diff --check`. O aceite deve terminar com `ok: QEMU raspi3b iniciou kernel8.img duas vezes e o console respondeu`.

---

## Task 7: Aceite no Raspberry Pi 3B+

**Files:**
- Modify: `agent.md`
- Modify: `README.md`

- [ ] **Step 1: Atualizar a mídia**

Confirmar `findmnt -n -o SOURCE,FSTYPE,TARGET /media/vladimir/PENDRIVE` como `/dev/sda1 vfat /media/vladimir/PENDRIVE`, verificar que não é NVMe e executar `make write DEST=/media/vladimir/PENDRIVE`.

- [ ] **Step 2: Testar boot e logs**

No Pi, confirmar banner do Marco 20, `logs`, `limites`, `fsck` e retorno ao prompt após falha controlada de leitura.

- [ ] **Step 3: Testar persistência e recuperação**

Executar `stress`, desligar/reiniciar em pelo menos três pontos definidos do journal e confirmar que arquivos confirmados permanecem legíveis e que o RPFS não sai da região.

- [ ] **Step 4: Registrar o aceite**

Marcar o Marco 20 como feito somente com evidência de teste automatizado, QEMU e Pi físico. Se algum teste físico falhar, registrar o sintoma e manter o marco aberto.

---

## Critério final de conclusão

O Marco 20 estará concluído somente quando:

1. os limites e falhas forem visíveis por logs e não causarem panic;
2. `fsck` detectar corrupção e recuperar casos não ambíguos;
3. RPFS v2 escolher superbloco válido e migrar RPFS v1 preservando dados;
4. o journal sobreviver a interrupções em todos os pontos testados;
5. a carga prolongada passar no QEMU sem inconsistência;
6. o mesmo ciclo de boot, armazenamento, recuperação e reinício passar no Raspberry Pi 3B+.

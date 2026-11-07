# Relatório da Task 4 — Marco 18

## Status

DONE — documentação e registro de aceite atualizados.

## Commit

`docs: registra aceite do marco 18` (hash informado no handoff final).

## Arquivos

- `agent.md`: Marco 18 registrado como QEMU feito e Pi aberto, com cena,
  resolução, formato, stride e comando de reprodução.
- `README.md`: seção do Marco 18 e reprodução visual do QEMU adicionadas.
- `.superpowers/sdd/task-4-report.md`: este relatório.
- `.cursor/skills/rasppios-bcm2837/registradores.md`: não alterado; não houve
  fato novo de registrador efetivamente usado nesta tarefa.

## Testes e validações

- `make check`: iniciado uma vez, mas ficou preso em
  `bash scripts/check-qemu.sh` nesta execução do ambiente; foi encerrado sem
  resultado final. Não foi executado novamente.
- `git diff --check`: passou.
- Lint do editor para os arquivos documentais: nenhum erro.
- `findmnt -n -o SOURCE,FSTYPE,TARGET /media/vladimir/PENDRIVE`: confirmou
  `/dev/sda1 vfat /media/vladimir/PENDRIVE`.
- Reprodução visual QEMU: confirmada anteriormente nas Tasks 1–3, com
  `video: 640x480 rgb565 stride 1280` e `tela: ok 640x480 rgb565`.

## Cópia ao cartão

Sim. `make write DEST=/media/vladimir/PENDRIVE` concluiu com sucesso, sem
formatar, copiando os cinco arquivos de boot para `/dev/sda1` em FAT32.

## Concerns

- O framebuffer ainda não foi confirmado no monitor de um Raspberry Pi 3B+
  físico; o aceite do Pi permanece aberto.
- A execução atual de `make check` não produziu resultado final porque o
  processo QEMU/harness ficou preso no ambiente; isso deve ser revalidado em
  um ambiente em que o encerramento do QEMU funcione.

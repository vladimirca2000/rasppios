#!/usr/bin/env bash
# Aceite no QEMU: a imagem raw sobe duas vezes e o console da miniUART
# responde a comandos enviados depois do prompt.
set -euo pipefail
cd "$(dirname "$0")/.."

IMG="boot/kernel8.img"
[[ -f "$IMG" ]] || { echo "erro: falta $IMG (rode make kernel)"; exit 1; }
RUN_DIR="$(mktemp -d "${TMPDIR:-/tmp}/rasppios-check.XXXXXX")"
export RASPPIOS_CHECK_DIR="$RUN_DIR"

boot_once() {
    local out="$1"
    set +e
    timeout --foreground -k 2 5 qemu-system-aarch64 \
        -M raspi3b \
        -kernel "$IMG" \
        -serial null \
        -serial stdio \
        -monitor none \
        -display none \
        -no-reboot >"$out" 2>&1
    set -e
    grep -q "rasppios: marco 23" "$out" \
        && grep -q "mmu: ligada" "$out" \
        && grep -q "nucleo em execucao: 0" "$out" \
        && grep -q "chegada: 0=1" "$out" \
        && grep -q "kernel somente no nucleo 0" "$out" \
        && grep -q "timer: 100 Hz" "$out" \
        && ! grep -q "ERRO nucleo" "$out"
}

# Espera o prompt num PTY e manda help, el, nucleo, eco e um comando invalido.
console_once() {
    local out="$1"
    set +e
    timeout --foreground -k 2 30 python3 - "$out" <<'PY'
import os, pty, re, select, socket, subprocess, sys, time

out_path = sys.argv[1]
img = "boot/kernel8.img"
run_dir = os.environ["RASPPIOS_CHECK_DIR"]
mon_path = os.path.join(run_dir, "rasppios-mon.sock")
sd_path = os.path.join(run_dir, "rasppios-sd.img")
try:
    os.unlink(mon_path)
except FileNotFoundError:
    pass
with open(sd_path, "wb") as image:
    image.truncate(4 * 1024 * 1024)
master, slave = pty.openpty()
proc = subprocess.Popen(
    [
        "qemu-system-aarch64",
        "-M", "raspi3b",
        "-kernel", img,
        "-serial", "null",
        "-serial", "stdio",
        "-display", "none",
        "-drive", f"if=sd,format=raw,file={sd_path}",
        "-device", "usb-kbd",
        "-monitor", f"unix:{mon_path},server,nowait",
        "-no-reboot",
    ],
    stdin=slave,
    stdout=slave,
    stderr=slave,
    close_fds=True,
)
os.close(slave)

buf = b""

def read_until(token, timeout=5):
    global buf
    end = time.time() + timeout
    while token not in buf:
        if time.time() > end or proc.poll() is not None:
            sys.stderr.write(buf.decode("utf-8", "replace"))
            raise SystemExit(f"timeout esperando {token!r}")
        ready, _, _ = select.select([master], [], [], 0.2)
        if ready:
            buf += os.read(master, 4096)

try:
    read_until(b"rasppios>")
    read_until(b"teclado: hid")
    mon = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    mon.settimeout(0.3)
    mon.connect(mon_path)
    time.sleep(0.1)
    try:
        mon.recv(512)
    except socket.timeout:
        pass

    def tap(name):
        mon.send(f"sendkey {name} 250\n".encode())
        time.sleep(0.4)

    tap("tab")
    read_until(b"gui: foco eventos")
    tap("tab")
    read_until(b"gui: foco status")
    tap("tab")
    read_until(b"gui: foco terminal")
    for name in ("e", "c", "h", "o", "spc", "o", "i", "ret"):
        tap(name)
    read_until(b"gui: echo oi")
    read_until(b"gui: saida oi")
    os.write(master, b"help\r")
    read_until(b"comandos: help ls cat echo run el nucleo eco ticks mem pagina trava tarefas preempcao usuario chamada bloco arquivos fsck stress verifica roda falha panico")
    os.write(master, b"tela\r")
    read_until(b"tela: ok 1920x1080 rgb565")
    os.write(master, b"el\r")
    read_until(b"el: EL1")
    os.write(master, b"nucleo\r")
    read_until(b"nucleo: 0 chegada: 0=1 1=0 2=0 3=0")
    os.write(master, b"eco qemu\r")
    read_until(b"eco: qemu")
    os.write(master, b"versao\r")
    read_until(b"versao: rasppios marco 23")
    read_until(b"recuperacao:")
    os.write(master, b"gfx\r")
    read_until(b"gfx: contrato ok")
    # O boot tenta 1920x1080, cai para 1280x720 e por fim 640x480. No QEMU
    # com 4 MiB de cartão o Full HD é entregue, então o modo ativo é esse.
    os.write(master, b"resolucao\r")
    read_until(b"resolucao: atual 1920x1080")
    # Regressão: `resolucao N` precisa funcionar logo após o boot, sem que
    # `modos` tenha rodado antes. Era esse o bug observado no Pi: a máscara
    # só era preenchida por `discover()`, então a troca real falhava.
    os.write(master, b"resolucao 9\r")
    read_until(b"resolucao: modo nao suportado")
    # Troca real antes de `modos`: aplica o modo 1 (800x600) e confirma.
    os.write(master, b"resolucao 1\r")
    read_until(b"resolucao: 800x600 aplicada")
    os.write(master, b"ok\r")
    read_until(b"resolucao: confirmada 800x600")
    os.write(master, b"resolucao\r")
    read_until(b"resolucao: atual 800x600")
    # Volta ao modo seguro e cancela, que deve restaurar 800x600.
    os.write(master, b"resolucao 0\r")
    read_until(b"resolucao: 640x480 aplicada")
    os.write(master, b"cancela\r")
    read_until(b"resolucao: modo anterior restaurado")
    os.write(master, b"resolucao\r")
    read_until(b"resolucao: atual 800x600")
    # Todos os modos da tabela precisam ser aplicáveis: antes, HD e Full HD
    # falhavam porque a janela do EL0 colidia com a faixa de framebuffer do
    # firmware (0x3C000000..0x3C280000).
    for index, size in [(2, b"1024x768"), (3, b"1280x720"), (4, b"1280x1024"), (5, b"1920x1080")]:
        os.write(master, f"resolucao {index}\r".encode())
        read_until(b"resolucao: " + size + b" aplicada", 12)
        os.write(master, b"ok\r")
        read_until(b"resolucao: confirmada " + size, 8)
    os.write(master, b"resolucao\r")
    read_until(b"resolucao: atual 1920x1080")
    # Volta ao modo seguro para o resto dos testes.
    os.write(master, b"resolucao 0\r")
    read_until(b"resolucao: 640x480 aplicada", 12)
    os.write(master, b"ok\r")
    read_until(b"resolucao: confirmada 640x480")
    # Expiração sem entrada: o modo anterior volta sozinho em CONFIRM_TICKS.
    os.write(master, b"resolucao 5\r")
    read_until(b"resolucao: 1920x1080 aplicada", 12)
    read_until(b"expirou; modo anterior restaurado", 15)
    os.write(master, b"resolucao\r")
    read_until(b"resolucao: atual 640x480")
    # Marco 25: trocas sucessivas não podem deixar artefatos nem perder a
    # tradução. Alterna modos várias vezes e confere que a GUI continua
    # desenhando e que o modo final é o esperado.
    for _ in range(3):
        for index, size in [(5, b"1920x1080"), (0, b"640x480")]:
            os.write(master, f"resolucao {index}\r".encode())
            read_until(b"resolucao: " + size + b" aplicada", 12)
            os.write(master, b"ok\r")
            read_until(b"resolucao: confirmada " + size, 8)
    os.write(master, b"resolucao 3\r")
    read_until(b"resolucao: 1280x720 aplicada", 12)
    os.write(master, b"ok\r")
    read_until(b"resolucao: confirmada 1280x720")
    os.write(master, b"tela\r")
    read_until(b"tela: ok 1280x720 rgb565", 10)
    # Só agora a descoberta, que é informativa.
    os.write(master, b"modos\r")
    read_until(b"modos: descoberta ok")
    read_until(b"modo[0]")
    os.write(master, b"naoexiste\r")
    read_until(b"desconhecido: naoexiste")

    def read_ticks():
        global buf
        mark = len(buf)
        os.write(master, b"ticks\r")
        end = time.time() + 3
        while time.time() < end and proc.poll() is None:
            ready, _, _ = select.select([master], [], [], 0.2)
            if ready:
                buf += os.read(master, 4096)
            found = re.findall(rb"ticks: (\d+) clo: (\d+)\r*\n", buf[mark:])
            if found:
                return int(found[-1][0]), int(found[-1][1])
        sys.stderr.write(buf.decode("utf-8", "replace"))
        raise SystemExit("timeout esperando ticks")

    n1, c1 = read_ticks()
    time.sleep(0.5)
    n2, c2 = read_ticks()
    dn = n2 - n1
    dc = (c2 - c1) & 0xFFFFFFFF
    expect = dc / 10000
    if dn < 30 or abs(dn - expect) > max(5, 0.25 * expect):
        sys.stderr.write(buf.decode("utf-8", "replace"))
        raise SystemExit(
            f"frequencia fora do esperado: ticks {n1}->{n2} clo {c1}->{c2}"
        )

    os.write(master, b"mem\r")
    read_until(b"ram: 0x00000000 0x3c000000 mailbox")
    read_until(b"mmio: 0x3f000000 0x40000000")
    read_until(b"sobrepostos: nao")
    read_until(b"liberar: ok")
    read_until(b"reuso: ok")
    heap = re.search(rb"heap: (0x[0-9a-f]+) (0x[0-9a-f]+)\r*\n", buf)
    blocks = re.findall(rb"bloco: (0x[0-9a-f]+) 4096\r*\n", buf)
    if heap is None or len(blocks) < 2:
        sys.stderr.write(buf.decode("utf-8", "replace"))
        raise SystemExit("mapa ou blocos ausentes")
    heap_lo = int(heap.group(1), 16)
    heap_hi = int(heap.group(2), 16)
    b1 = int(blocks[-2], 16)
    b2 = int(blocks[-1], 16)
    mmio_lo, mmio_hi = 0x3F000000, 0x40000000
    spans = ((b1, b1 + 4096), (b2, b2 + 4096))
    inside = all(heap_lo <= lo and hi <= heap_hi for lo, hi in spans)
    apart = b1 + 4096 <= b2 or b2 + 4096 <= b1
    clear = all(hi <= mmio_lo or lo >= mmio_hi for lo, hi in spans)
    if not (inside and apart and clear and heap_hi == 0x3C000000):
        sys.stderr.write(buf.decode("utf-8", "replace"))
        raise SystemExit(
            f"alocacao invalida: heap {heap_lo:#x}-{heap_hi:#x} blocos {b1:#x} {b2:#x}"
        )

    os.write(master, b"pagina\r")
    read_until(b"pagina: ok 0xa5a5a5a5a5a5a5a5")
    read_until(b"causa: abort de dados")
    read_until(b"esr: 0x96000006")
    read_until(b"far: 0x000000003e000000")
    read_until(b"pagina: falha contida")

    os.write(master, b"trava\r")
    read_until(b"trava: nucleos 4 soma 4000")

    os.write(master, b"tarefas\r")
    read_until(b"tarefas: a 4 b 4")
    read_until(b"ordem: abababab")
    read_until(b"preservado: ok")

    os.write(master, b"preempcao\r")
    read_until(b"preempcao: alta antes")
    read_until(b"fatias: abababab")

    os.write(master, b"usuario\r")
    read_until(b"usuario: el0")
    read_until(b"causa: abort de dados")
    read_until(b"esr: 0x9200000f")
    read_until(b"far: 0x0000000000080000")
    read_until(b"proprio: ok")
    read_until(b"modo: EL0")
    read_until(b"usuario: kernel negado")

    os.write(master, b"chamada\r")
    read_until(b"saida: ok")
    os.write(master, b"Z")
    read_until(b"ponteiro: negado")
    read_until(b"entrada: Z")
    read_until(b"fim: 0")
    tempo = re.search(rb"tempo: (\d+)\r*\n", buf)
    if tempo is None or int(tempo.group(1)) < c2:
        sys.stderr.write(buf.decode("utf-8", "replace"))
        raise SystemExit(f"tempo invalido depois de clo {c2}")

    mark = len(buf)
    end = time.time() + 3
    while b"rasppios>" not in buf[mark:] and time.time() < end and proc.poll() is None:
        ready, _, _ = select.select([master], [], [], 0.2)
        if ready:
            buf += os.read(master, 4096)
    mon.close()

    os.write(master, b"bloco\r")
    read_until(b"leitura: 00000000")
    read_until(b"escrita: ok")
    read_until(b"restauro: ok")

    os.write(master, b"arquivos\r")
    read_until(b"montagem: nova")
    read_until(b"lista: ola notas")
    read_until(b"leitura: rasppios")
    read_until(b"sub: dentro")
    os.write(master, b"fsck\r")
    read_until(b"fsck: ok")
    os.write(master, b"stress\r")
    read_until(b"stress: ok 100")
    os.write(master, b"verifica\r")
    read_until(b"fsck: ok")
    read_until(b"stress: ok 100")
    os.write(master, b"falha leitura\r")
    read_until(b"log W: sd-fault-configured")
    os.write(master, b"bloco\r")
    read_until(b"bloco: falhou")
    os.write(master, b"falha escrita\r")
    read_until(b"log W: sd-fault-configured")
    os.write(master, b"bloco\r")
    read_until(b"bloco: falhou")
    os.write(master, b"falha apos 1\r")
    read_until(b"log W: sd-fault-configured")
    os.write(master, b"bloco\r")
    read_until(b"leitura: 00000000")
    read_until(b"log E: sd-read-fault")
    read_until(b"bloco: falhou")

    os.write(master, b"roda\r")
    read_until(b"roda: gravou")
    read_until(b"app: ok")
    read_until(b"saida: 7")

    os.write(master, b"ls\r")
    read_until(b"ls: ola notas prog")
    os.write(master, b"ls notas\r")
    read_until(b"ls: item")
    os.write(master, b"cat ola\r")
    read_until(b"cat: rasppios")
    os.write(master, b"cat notas/item\r")
    read_until(b"cat: dentro")
    os.write(master, b"echo shell\r")
    read_until(b"echo: shell")
    os.write(master, b"run prog\r")
    read_until(b"run: prog")
    read_until(b"run: ok 7")

    os.write(master, b"falha\r")
    read_until(b"excecao: sincrono")
    read_until(b"causa: brk")
    read_until(b"esr: 0xf2000001")
    read_until(b"x0: 0x0000000000000041")
    os.write(master, b"panico\r")
    read_until(b"PANIC")
    read_until(b"provocado")
finally:
    proc.terminate()
    try:
        proc.wait(timeout=2)
    except subprocess.TimeoutExpired:
        proc.kill()
    text = buf.decode("utf-8", "replace")
    with open(out_path, "w", encoding="utf-8") as fh:
        fh.write(text)
    os.close(master)
PY
    local status=$?
    set -e
    return "$status"
}

persist_once() {
    local out="$1"
    set +e
    timeout --foreground -k 2 20 python3 - "$out" <<'PY'
import os, pty, select, subprocess, sys, time

out_path = sys.argv[1]
sd_path = os.path.join(os.environ["RASPPIOS_CHECK_DIR"], "rasppios-sd.img")
master, slave = pty.openpty()
proc = subprocess.Popen(
    [
        "qemu-system-aarch64",
        "-M", "raspi3b",
        "-kernel", "boot/kernel8.img",
        "-serial", "null",
        "-serial", "stdio",
        "-display", "none",
        "-drive", f"if=sd,format=raw,file={sd_path}",
        "-monitor", "none",
        "-no-reboot",
    ],
    stdin=slave,
    stdout=slave,
    stderr=slave,
    close_fds=True,
)
os.close(slave)
buf = b""

def read_until(token, timeout=8):
    global buf
    end = time.time() + timeout
    while token not in buf:
        if time.time() > end or proc.poll() is not None:
            sys.stderr.write(buf.decode("utf-8", "replace"))
            raise SystemExit(f"timeout esperando {token!r}")
        ready, _, _ = select.select([master], [], [], 0.2)
        if ready:
            buf += os.read(master, 4096)

try:
    read_until(b"rasppios>")
    os.write(master, b"arquivos\r")
    read_until(b"montagem: pronta")
    read_until(b"lista: ola notas")
    read_until(b"leitura: rasppios")
    read_until(b"sub: dentro")
    read_until(b"persistiu: ok")
    os.write(master, b"roda\r")
    read_until(b"roda: disco")
    read_until(b"app: ok")
    read_until(b"saida: 7")
finally:
    proc.terminate()
    try:
        proc.wait(timeout=2)
    except subprocess.TimeoutExpired:
        proc.kill()
    text = buf.decode("utf-8", "replace")
    with open(out_path, "w", encoding="utf-8") as fh:
        fh.write(text)
    os.close(master)
PY
    local status=$?
    set -e
    return "$status"
}

# O número do marco tem uma única fonte em `src/version.rs`. Este teste falha
# se algum lugar ainda tiver o literal antigo ou se o banner divergir do
# `versao` que o console imprime.
version_once() {
    local source
    source=$(sed -n 's/^pub const MARCO: u8 = \([0-9]\+\).*/\1/p' src/version.rs)
    [[ -n $source ]] || { echo "erro: MARCO ausente em src/version.rs"; return 1; }

    # Nenhum título "rasppios marco <numero>" com número diferente do atual.
    local stale
    stale=$(grep -rnE 'rasppios marco [0-9]+' src/ | grep -v "rasppios marco ${source}" | grep -v "MARCO: u8" || true)
    if [[ -n $stale ]]; then
        echo "erro: literal de marco divergente de ${source}:"
        echo "$stale"
        return 1
    fi

    # A prévia anuncia o mesmo número que o código.
    bash scripts/preview.sh --help >/dev/null 2>&1 || true
    local preview
    preview=$(grep -oE "rasppios marco \\\$MARCO" scripts/preview.sh | head -1 || true)
    [[ -n $preview ]] || { echo "erro: preview.sh nao usa \$MARCO"; return 1; }

    echo "versao: marco ${source} coerente em src/, preview e testes"
    return 0
}

dist_once() {
    local out="$1"
    local img="${RUN_DIR}/rasppios-dist.img"
    if ! bash scripts/dist-image.sh "$img" >"$out" 2>&1; then
        return 1
    fi
    local offset=1048576
    local part_logical=$((2048 * 512))
    grep -q "partição inicializável" "$out" || return 1
    grep -q "start= *2048, size=.*type=c, bootable" <(sfdisk -d "$img") || return 1
    local first
    first=$(dd if="$img" bs=512 skip=0 count=1 status=none | od -An -tx1 -j446 -N16 | tr -d ' \n')
    [[ ${first:8:2} == "0c" ]] || return 1
    [[ ${first:0:2} == "80" ]] || return 1
    for f in bootcode.bin start.elf fixup.dat config.txt kernel8.img; do
        MTOOLS_SKIP_CHECK=1 mdir -i "$img@@$offset" "::$f" >/dev/null 2>&1 || return 1
    done
    cmp -s boot/kernel8.img <(MTOOLS_SKIP_CHECK=1 mtype -i "$img@@$offset" ::kernel8.img) || return 1
    return 0
}

OUT1="$(mktemp)"
OUT2="$(mktemp)"
OUT3="$(mktemp)"
OUT4="$(mktemp)"
OUT5="$(mktemp)"
cleanup() {
    rm -f "$OUT1" "$OUT2" "$OUT3" "$OUT4" "$OUT5"
    rm -rf "$RUN_DIR"
}
trap cleanup EXIT

if ! boot_once "$OUT1" || ! boot_once "$OUT2"; then
    echo "falha: boot do nucleo 0 nao repetiu no QEMU raspi3b"
    echo "---- primeira ----"
    cat "$OUT1"
    echo "---- segunda ----"
    cat "$OUT2"
    exit 1
fi

if ! console_once "$OUT3"; then
    echo "falha: console serial nao respondeu aos comandos"
    echo "---- console ----"
    cat "$OUT3"
    exit 1
fi

if ! persist_once "$OUT4"; then
    echo "falha: o cartao nao preservou os arquivos depois de reiniciar"
    echo "---- reinicio ----"
    cat "$OUT4"
    exit 1
fi

echo "ok: QEMU raspi3b iniciou kernel8.img duas vezes e o console respondeu"
cat "$OUT3"
echo "---- reinicio ----"
cat "$OUT4"

if ! version_once; then
    echo "falha: o numero do marco nao esta coerente entre codigo, preview e testes"
    exit 1
fi

if ! dist_once "$OUT5"; then
    echo "falha: a imagem de distribuicao nao ficou inicializavel"
    echo "---- distrib ----"
    cat "$OUT5"
    exit 1
fi

echo "---- distribuicao ----"
cat "$OUT5"
echo "ok: imagem de distribuicao com particao FAT32 inicializavel e os 5 arquivos"

use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};

use crate::{log, usb, uart, video};

const WIDTH: i32 = 640;
/// Tamanho mínimo para o layout de três painéis caber sem sobreposição.
const MIN_WIDTH: i32 = 640;
const MIN_HEIGHT: i32 = 480;
const TITLE_H: i32 = 24;
const MAX_LINE: usize = 48;
const MAX_EVENTS: usize = 2;

/// Linhas de histórico do terminal gráfico, independentes da resolução.
const TERMINAL_LINES: usize = 16;

/// Altura da linha de texto na fonte 5x8.
const LINE_H: i32 = 16;
/// Recuo interno entre a borda do painel e o texto.
const PAD: i32 = 12;
/// Largura de um caractere com avanço de 6 px.
const CHAR_W: i32 = 6;

/// Painéis do desktop, calculados a partir do tamanho real da tela.
///
/// Em 640x480 o resultado é exatamente o layout original: verde de 200x240,
/// azul de 416x240 e vermelho de 624x192. Em resoluções maiores os painéis
/// crescem juntos, mantendo proporção e o mesmo espaçamento de 8 px.
#[derive(Clone, Copy)]
struct Layout {
    title_w: i32,
    status: (i32, i32, i32, i32),
    term: (i32, i32, i32, i32),
    events: (i32, i32, i32, i32),
    term_cols: usize,
    events_cols: usize,
    terminal_lines: usize,
    log_lines: usize,
}

impl Layout {
    fn for_screen(width: i32, height: i32) -> Self {
        let margin = 8;
        let gap = 8;
        let title_h = TITLE_H;
        // Espaço vertical útil depois da barra e do separador.
        let usable_h = height - title_h - 2 - margin * 2;
        // A faixa superior ocupa ~55% e a inferior o resto, como em 640x480.
        let top_h = usable_h * 55 / 100;
        let events_h = usable_h - top_h - gap;
        let status_w = width * 200 / WIDTH;
        let top_y = title_h + 2 + margin;
        let status = (margin, top_y, status_w, top_h);
        let term = (margin + status_w + gap, top_y, width - status_w - gap - margin * 2, top_h);
        let events = (margin, top_y + top_h + gap, width - margin * 2, events_h);

        let cols = |w: i32| ((w - 2 * PAD) / CHAR_W).max(1) as usize;
        let lines = |h: i32| ((h - PAD * 2 - LINE_H) / LINE_H).max(1) as usize;

        Self {
            title_w: width,
            status,
            term,
            events,
            term_cols: cols(term.2),
            events_cols: cols(events.2),
            terminal_lines: lines(term.3).min(16),
            log_lines: lines(events.3).saturating_sub(MAX_EVENTS + 1).min(8),
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum Window {
    Status,
    Terminal,
    Events,
}

#[derive(Clone, Copy)]
struct State {
    ready: bool,
    focus: Window,
    line: [u8; MAX_LINE],
    line_len: usize,
    terminal: [[u8; MAX_LINE]; TERMINAL_LINES],
    terminal_len: [usize; TERMINAL_LINES],
    events: [[u8; MAX_LINE]; MAX_EVENTS],
    event_len: [usize; MAX_EVENTS],
    log_generation: usize,
}

static mut STATE: State = State {
    ready: false,
    focus: Window::Terminal,
    line: [0; MAX_LINE],
    line_len: 0,
    terminal: [[0; MAX_LINE]; TERMINAL_LINES],
    terminal_len: [0; TERMINAL_LINES],
    events: [[0; MAX_LINE]; MAX_EVENTS],
    event_len: [0; MAX_EVENTS],
    log_generation: 0,
};

pub fn init() -> bool {
    let (width, height) = video::dimensions();
    let mode = video::active();
    if !video::available()
        || width < MIN_WIDTH
        || height < MIN_HEIGHT
        || mode.is_none()
    {
        return false;
    }
    unsafe {
        let state = addr_of_mut!(STATE);
        (*state).ready = true;
        write_volatile(addr_of_mut!((*state).focus), Window::Terminal);
        write_volatile(addr_of_mut!((*state).line_len), 0);
        (*state).line = [0; MAX_LINE];
        (*state).terminal = [[0; MAX_LINE]; TERMINAL_LINES];
        (*state).terminal_len = [0; TERMINAL_LINES];
        (*state).events = [[0; MAX_LINE]; MAX_EVENTS];
        (*state).event_len = [0; MAX_EVENTS];
        (*state).log_generation = log::generation();
    }
    true
}

pub fn ready() -> bool {
    unsafe { read_volatile(addr_of!(STATE.ready)) }
}

pub fn render() {
    let state = unsafe { read_volatile(addr_of!(STATE)) };
    if !state.ready {
        return;
    }

    let (screen_w, screen_h) = video::dimensions();
    let layout = Layout::for_screen(screen_w, screen_h);

    video::clear(video::NAVY);
    video::rect(0, 0, layout.title_w, TITLE_H, video::BLUE);
    // Preto como separador de 2 px entre os painéis, definindo o contraste.
    video::rect(0, TITLE_H, layout.title_w, 2, video::BLACK);

    let status_color = video::GREEN;
    let terminal_color = video::BLUE;
    let events_color = video::RED;
    let (sx, sy, sw, sh) = layout.status;
    let (tx, ty, tw, th) = layout.term;
    let (ex, ey, ew, eh) = layout.events;
    video::rect(sx, sy, sw, sh, status_color);
    video::rect(tx, ty, tw, th, terminal_color);
    video::rect(ex, ey, ew, eh, events_color);

    let mut title_buf = [0u8; 24];
    let title = crate::version::title(&mut title_buf);
    video::text(16, 8, title, video::WHITE, video::BLUE);
    video::text(sx + PAD, sy + PAD, "status", video::WHITE, status_color);
    video::text(tx + PAD, ty + PAD, "terminal", video::WHITE, terminal_color);
    video::text(ex + PAD, ey + PAD, "eventos", video::WHITE, events_color);

    let mut y = sy + PAD + LINE_H;
    status_line(&mut y, layout.status, status_color, b"EL1");
    let mut video_line = [0u8; 32];
    let mut vlen = copy_prefix(&mut video_line, b"video ");
    vlen += decimal(&mut video_line[vlen..], screen_w as u32);
    video_line[vlen] = b'x';
    vlen += 1;
    vlen += decimal(&mut video_line[vlen..], screen_h as u32);
    status_line(&mut y, layout.status, status_color, &video_line[..vlen]);
    status_line(&mut y, layout.status, status_color, b"UART ativa");
    status_line(&mut y, layout.status, status_color, b"robustez: logs");
    status_line(&mut y, layout.status, status_color, b"fsck journal");
    status_line(&mut y, layout.status, status_color, b"carga: 100 ciclos");
    status_line(&mut y, layout.status, status_color, b"v2: superblocos");
    status_line(&mut y, layout.status, status_color, b"journal: ativo");
    let (hprt, speed) = usb::status();
    if hprt != 0 {
        let mut line = [0u8; 40];
        let mut len = copy_prefix(&mut line, b"usb ");
        len += hex(&mut line[len..], hprt);
        len += copy_prefix(&mut line[len..], b" sp ");
        line[len] = b'0' + (speed as u8 % 10);
        len += 1;
        status_line(&mut y, layout.status, status_color, &line[..len]);
    }
    let (ints, char0) = usb::diag();
    if ints != 0 || char0 != 0 {
        let mut line = [0u8; 40];
        let mut len = copy_prefix(&mut line, b"hc ");
        len += hex(&mut line[len..], ints);
        len += copy_prefix(&mut line[len..], b" c ");
        len += hex(&mut line[len..], char0);
        status_line(&mut y, layout.status, status_color, &line[..len]);
    }
    let (gcfg, hcfg) = usb::cfg();
    if gcfg != 0 || hcfg != 0 {
        let mut line = [0u8; 40];
        let mut len = copy_prefix(&mut line, b"cfg ");
        len += hex(&mut line[len..], gcfg);
        len += copy_prefix(&mut line[len..], b" h ");
        len += hex(&mut line[len..], hcfg);
        status_line(&mut y, layout.status, status_color, &line[..len]);
    }
    let (frames, timeouts) = usb::timing();
    if frames != 0 || timeouts != 0 {
        let mut line = [0u8; 40];
        let mut len = copy_prefix(&mut line, b"fr ");
        len += hex(&mut line[len..], frames);
        len += copy_prefix(&mut line[len..], b" to ");
        len += hex(&mut line[len..], timeouts);
        status_line(&mut y, layout.status, status_color, &line[..len]);
    }

    // Terminal: as últimas `terminal_lines` linhas do histórico.
    let term_top = ty + PAD + LINE_H;
    let visible = layout.terminal_lines.min(TERMINAL_LINES);
    let first = TERMINAL_LINES - visible;
    for (offset, index) in (first..TERMINAL_LINES).enumerate() {
        if state.terminal_len[index] != 0 {
            let len = state.terminal_len[index].min(layout.term_cols);
            draw_bytes(
                tx + PAD,
                term_top + offset as i32 * LINE_H,
                &state.terminal[index][..len],
                video::WHITE,
                terminal_color,
            );
        }
    }
    let mut prompt = [0u8; MAX_LINE];
    let prompt_prefix = b"rasppios gui> ";
    let mut prompt_len = 0;
    for byte in prompt_prefix {
        prompt[prompt_len] = *byte;
        prompt_len += 1;
    }
    for byte in &state.line[..state.line_len] {
        if prompt_len == prompt.len() {
            break;
        }
        prompt[prompt_len] = *byte;
        prompt_len += 1;
    }
    draw_bytes(
        tx + PAD,
        term_top + (visible as i32 - 1) * LINE_H,
        &prompt[..prompt_len],
        video::WHITE,
        terminal_color,
    );

    draw_focus(state.focus, layout);
    let events_top = ey + PAD + LINE_H;
    video::text(ex + PAD, events_top, "foco:", video::WHITE, events_color);
    let focus_x = ex + PAD + 36;
    match state.focus {
        Window::Status => video::text(focus_x, events_top, "status", video::WHITE, events_color),
        Window::Terminal => video::text(focus_x, events_top, "terminal", video::WHITE, events_color),
        Window::Events => video::text(focus_x, events_top, "eventos", video::WHITE, events_color),
    }
    let mut events_y = events_top + LINE_H;
    for index in 0..MAX_EVENTS {
        if state.event_len[index] != 0 {
            let len = state.event_len[index].min(layout.events_cols);
            draw_bytes(
                ex + PAD,
                events_y,
                &state.events[index][..len],
                video::WHITE,
                events_color,
            );
        }
        events_y += LINE_H;
    }
    let mut logs = [[0u8; log::EVENT_LEN]; log::DISPLAY_EVENTS];
    let log_lengths = log::snapshot(&mut logs);
    video::text(ex + PAD, events_y, "logs:", video::WHITE, events_color);
    events_y += LINE_H;
    // Só os mais recentes cabem; `snapshot` devolve em ordem cronológica.
    let want_lines = layout.log_lines.min(log::DISPLAY_EVENTS);
    let first_log = log::DISPLAY_EVENTS - want_lines;
    for index in first_log..log::DISPLAY_EVENTS {
        if log_lengths[index] != 0 {
            let len = log_lengths[index].min(layout.events_cols);
            draw_bytes(
                ex + PAD,
                events_y,
                &logs[index][..len],
                video::WHITE,
                events_color,
            );
        }
        events_y += LINE_H;
    }

    let _ = video::flush();
}

/// Desenha uma linha do painel de status se ainda couber na área colorida.
fn status_line(y: &mut i32, area: (i32, i32, i32, i32), background: u16, bytes: &[u8]) {
    let (x, top, width, height) = area;
    if *y + 8 > top + height - PAD {
        return;
    }
    let cols = ((width - 2 * PAD) / CHAR_W).max(1) as usize;
    let len = bytes.len().min(cols);
    draw_bytes(x + PAD, *y, &bytes[..len], video::WHITE, background);
    *y += LINE_H;
}

/// Escreve `value` em decimal e devolve quantos bytes usou.
fn decimal(destination: &mut [u8], value: u32) -> usize {
    let mut digits = [0u8; 10];
    let mut count = 0;
    let mut left = value;
    loop {
        digits[count] = b'0' + (left % 10) as u8;
        count += 1;
        left /= 10;
        if left == 0 {
            break;
        }
    }
    let used = count.min(destination.len());
    for index in 0..used {
        destination[index] = digits[count - 1 - index];
    }
    used
}

pub fn poll() -> bool {
    if !ready() {
        return false;
    }
    if let Some(byte) = usb::poll() {
        return unsafe { handle_byte(byte) };
    }
    let generation = log::generation();
    unsafe {
        let state = &mut *addr_of_mut!(STATE);
        if state.log_generation != generation {
            state.log_generation = generation;
            return true;
        }
    }
    false
}

unsafe fn handle_byte(byte: u8) -> bool {
    let state = &mut *addr_of_mut!(STATE);
    match byte {
        b'\t' => {
            state.focus = match state.focus {
                Window::Status => Window::Terminal,
                Window::Terminal => Window::Events,
                Window::Events => Window::Status,
            };
            let name = focus_name(state.focus);
            record_event(state, name.as_bytes());
            uart::write_fmt(format_args!("gui: foco {}\r\n", name));
            true
        }
        0x7f | 0x08 => {
            if matches!(state.focus, Window::Terminal) && state.line_len > 0 {
                state.line_len -= 1;
                true
            } else {
                record_event(state, b"backspace");
                true
            }
        }
        b'\r' | b'\n' => {
            if matches!(state.focus, Window::Terminal) {
                execute_line(state);
            } else {
                record_event(state, b"enter");
            }
            true
        }
        0x20..=0x7e => {
            if matches!(state.focus, Window::Terminal) {
                if state.line_len < MAX_LINE {
                    state.line[state.line_len] = byte;
                    state.line_len += 1;
                }
            } else {
                record_event_byte(state, byte);
            }
            true
        }
        _ => false,
    }
}

fn execute_line(state: &mut State) {
    let line_len = state.line_len;
    let mut line = [0u8; MAX_LINE];
    line[..line_len].copy_from_slice(&state.line[..line_len]);
    record_event(state, &line[..line_len]);
    let mut command = [0u8; MAX_LINE];
    let command_len = copy_prefix(&mut command, b"rasppios gui> ");
    let command_len = command_len + copy_prefix(&mut command[command_len..], &line[..line_len]);
    add_terminal_line(state, &command[..command_len]);

    let text = core::str::from_utf8(&line[..line_len]).unwrap_or("");
    if text == "clear" {
        state.terminal = [[0; MAX_LINE]; TERMINAL_LINES];
        state.terminal_len = [0; TERMINAL_LINES];
    } else if text == "help" {
        gui_output(state, b"help clear echo");
    } else if let Some(rest) = text.strip_prefix("echo ") {
        gui_output(state, rest.as_bytes());
        uart::write_fmt(format_args!("gui: echo {}\r\n", rest));
        uart::write_fmt(format_args!("gui: saida {}\r\n", rest));
    } else if text.is_empty() {
        // Keep the prompt interaction local without adding an empty response.
    } else {
        gui_output(state, b"gui: comando desconhecido");
        uart::write_fmt(format_args!("gui: comando desconhecido\r\n"));
    }
    state.line = [0; MAX_LINE];
    state.line_len = 0;
}

fn gui_output(state: &mut State, output: &[u8]) {
    add_terminal_line(state, output);
}

fn add_terminal_line(state: &mut State, line: &[u8]) {
    for index in 1..TERMINAL_LINES {
        state.terminal[index - 1] = state.terminal[index];
        state.terminal_len[index - 1] = state.terminal_len[index];
    }
    let last = TERMINAL_LINES - 1;
    state.terminal[last] = [0; MAX_LINE];
    state.terminal_len[last] = copy_prefix(&mut state.terminal[last], line);
}

fn record_event(state: &mut State, event: &[u8]) {
    for index in 1..MAX_EVENTS {
        state.events[index - 1] = state.events[index];
        state.event_len[index - 1] = state.event_len[index];
    }
    let last = MAX_EVENTS - 1;
    state.events[last] = [0; MAX_LINE];
    state.event_len[last] = copy_prefix(&mut state.events[last], event);
}

fn record_event_byte(state: &mut State, byte: u8) {
    let mut event = [0u8; 1];
    event[0] = byte;
    record_event(state, &event);
}

fn copy_prefix(destination: &mut [u8], source: &[u8]) -> usize {
    let length = destination.len().min(source.len());
    destination[..length].copy_from_slice(&source[..length]);
    length
}

/// Escreve `value` em hexadecimal minúsculo e devolve quantos bytes usou.
fn hex(destination: &mut [u8], value: u32) -> usize {
    let mut used = 0;
    let mut started = false;
    for shift in (0..8).rev() {
        let digit = ((value >> (shift * 4)) & 0xf) as u8;
        if digit == 0 && !started && shift != 0 {
            continue;
        }
        started = true;
        if used == destination.len() {
            break;
        }
        destination[used] = if digit < 10 { b'0' + digit } else { b'a' + digit - 10 };
        used += 1;
    }
    used
}

fn focus_name(focus: Window) -> &'static str {
    match focus {
        Window::Status => "status",
        Window::Terminal => "terminal",
        Window::Events => "eventos",
    }
}

fn draw_focus(focus: Window, layout: Layout) {
    let (x, y, width, height) = match focus {
        Window::Status => layout.status,
        Window::Terminal => layout.term,
        Window::Events => layout.events,
    };
    let border = video::WHITE;
    video::rect(x, y, width, 2, border);
    video::rect(x, y + height - 2, width, 2, border);
    video::rect(x, y, 2, height, border);
    video::rect(x + width - 2, y, 2, height, border);
}

fn draw_bytes(x: i32, y: i32, bytes: &[u8], color: u16, background: u16) {
    if let Ok(text) = core::str::from_utf8(bytes) {
        video::text(x, y, text, color, background);
    }
}

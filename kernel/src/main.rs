#![no_std]
#![no_main]

use core::panic::PanicInfo;

type Status = usize;
type Handle = *mut core::ffi::c_void;

const EFI_SUCCESS: Status = 0;
const SCREEN_W: usize = 80;
const SCREEN_H: usize = 25;

const SCAN_UP: u16 = 0x01;
const SCAN_DOWN: u16 = 0x02;
const SCAN_RIGHT: u16 = 0x03;
const SCAN_LEFT: u16 = 0x04;
const SCAN_ESC: u16 = 0x17;

const ATTR_NORMAL: usize = 0x0f;
const ATTR_DIM: usize = 0x08;
const ATTR_FOCUS: usize = 0x1f;
const ATTR_INACTIVE: usize = 0x70;
const ATTR_STATUS: usize = 0x30;

#[repr(C)]
struct TableHeader {
    signature: u64,
    revision: u32,
    header_size: u32,
    crc32: u32,
    reserved: u32,
}

#[repr(C)]
pub struct SystemTable {
    header: TableHeader,
    firmware_vendor: *mut u16,
    firmware_revision: u32,
    console_in_handle: Handle,
    con_in: *mut SimpleTextInputProtocol,
    console_out_handle: Handle,
    con_out: *mut SimpleTextOutputProtocol,
    standard_error_handle: Handle,
    std_err: *mut SimpleTextOutputProtocol,
    runtime_services: *mut core::ffi::c_void,
    boot_services: *mut core::ffi::c_void,
    number_of_table_entries: usize,
    configuration_table: *mut core::ffi::c_void,
}

#[repr(C)]
struct InputKey {
    scan_code: u16,
    unicode_char: u16,
}

#[repr(C)]
struct SimpleTextInputProtocol {
    reset: unsafe extern "efiapi" fn(*mut SimpleTextInputProtocol, bool) -> Status,
    read_key_stroke: unsafe extern "efiapi" fn(*mut SimpleTextInputProtocol, *mut InputKey) -> Status,
    wait_for_key: *mut core::ffi::c_void,
}

#[repr(C)]
struct SimpleTextOutputProtocol {
    reset: unsafe extern "efiapi" fn(*mut SimpleTextOutputProtocol, bool) -> Status,
    output_string: unsafe extern "efiapi" fn(*mut SimpleTextOutputProtocol, *const u16) -> Status,
    test_string: usize,
    query_mode: usize,
    set_mode: usize,
    set_attribute: unsafe extern "efiapi" fn(*mut SimpleTextOutputProtocol, usize) -> Status,
    clear_screen: unsafe extern "efiapi" fn(*mut SimpleTextOutputProtocol) -> Status,
    set_cursor_position: unsafe extern "efiapi" fn(*mut SimpleTextOutputProtocol, usize, usize) -> Status,
    enable_cursor: unsafe extern "efiapi" fn(*mut SimpleTextOutputProtocol, bool) -> Status,
    mode: *mut core::ffi::c_void,
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[no_mangle]
pub extern "efiapi" fn efi_main(_image: Handle, system_table: *mut SystemTable) -> Status {
    let mut app = unsafe { App::new(system_table) };
    app.run();
    EFI_SUCCESS
}

struct Console {
    input: *mut SimpleTextInputProtocol,
    output: *mut SimpleTextOutputProtocol,
}

impl Console {
    unsafe fn new(system_table: *mut SystemTable) -> Self {
        Self {
            input: (*system_table).con_in,
            output: (*system_table).con_out,
        }
    }

    fn reset(&mut self) {
        unsafe {
            ((*self.output).reset)(self.output, false);
            ((*self.output).enable_cursor)(self.output, false);
            ((*self.output).set_attribute)(self.output, ATTR_NORMAL);
            ((*self.output).clear_screen)(self.output);
        }
    }

    fn clear(&mut self) {
        unsafe {
            ((*self.output).set_attribute)(self.output, ATTR_NORMAL);
            ((*self.output).clear_screen)(self.output);
        }
    }

    fn set_attr(&mut self, attr: usize) {
        unsafe {
            ((*self.output).set_attribute)(self.output, attr);
        }
    }

    fn write_at(&mut self, x: usize, y: usize, text: &str) {
        self.write_bytes_at(x, y, text.as_bytes());
    }

    fn write_bytes_at(&mut self, x: usize, y: usize, bytes: &[u8]) {
        if x >= SCREEN_W || y >= SCREEN_H {
            return;
        }

        let max = min(bytes.len(), min(SCREEN_W - x, 120));
        let mut utf16 = [0u16; 121];
        let mut i = 0;
        while i < max {
            let b = bytes[i];
            utf16[i] = if b.is_ascii_graphic() || b == b' ' { b as u16 } else { b'.' as u16 };
            i += 1;
        }
        utf16[max] = 0;

        unsafe {
            ((*self.output).set_cursor_position)(self.output, x, y);
            ((*self.output).output_string)(self.output, utf16.as_ptr());
        }
    }

    fn fill_at(&mut self, x: usize, y: usize, width: usize, ch: u8) {
        let mut line = [b' '; SCREEN_W];
        let len = min(width, SCREEN_W.saturating_sub(x));
        let mut i = 0;
        while i < len {
            line[i] = ch;
            i += 1;
        }
        self.write_bytes_at(x, y, &line[..len]);
    }

    fn read_key(&mut self) -> Option<InputKey> {
        let mut key = InputKey { scan_code: 0, unicode_char: 0 };
        let status = unsafe { ((*self.input).read_key_stroke)(self.input, &mut key) };
        if status == EFI_SUCCESS {
            Some(key)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Terminal,
    Files,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    Split,
    TerminalMax,
    FilesMax,
}

#[derive(Clone, Copy)]
struct Rect {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

struct App {
    console: Console,
    focus: Focus,
    layout: Layout,
    terminal: Terminal,
    files: FileBrowser,
}

impl App {
    unsafe fn new(system_table: *mut SystemTable) -> Self {
        let mut terminal = Terminal::new();
        terminal.add_line(b"RustOS POC UEFI shell. Type 'help'.");
        terminal.add_line(b"Tab switches windows. Esc maximizes/restores.");

        Self {
            console: Console::new(system_table),
            focus: Focus::Terminal,
            layout: Layout::Split,
            terminal,
            files: FileBrowser::new(),
        }
    }

    fn run(&mut self) {
        self.console.reset();
        self.render();

        loop {
            if let Some(key) = self.console.read_key() {
                self.handle_key(key);
                self.render();
            } else {
                core::hint::spin_loop();
            }
        }
    }

    fn handle_key(&mut self, key: InputKey) {
        if key.unicode_char == 9 {
            self.focus = match self.focus {
                Focus::Terminal => Focus::Files,
                Focus::Files => Focus::Terminal,
            };
            return;
        }

        if key.scan_code == SCAN_ESC {
            self.layout = match (self.layout, self.focus) {
                (Layout::Split, Focus::Terminal) => Layout::TerminalMax,
                (Layout::Split, Focus::Files) => Layout::FilesMax,
                _ => Layout::Split,
            };
            return;
        }

        match self.focus {
            Focus::Terminal => self.terminal.handle_key(key),
            Focus::Files => self.files.handle_key(key),
        }
    }

    fn render(&mut self) {
        self.console.clear();
        self.console.set_attr(ATTR_STATUS);
        self.console.fill_at(0, 0, SCREEN_W, b' ');
        self.console.write_at(1, 0, "RustOS POC - UEFI text desktop");
        self.console.write_at(45, 0, "Apps: Terminal + File Browser");

        self.console.set_attr(ATTR_DIM);
        self.console.write_at(1, 1, "Tab focus | Esc maximize/restore | Terminal: help, ls, cd, cat, clear | File Browser: arrows, Enter, Backspace");

        match self.layout {
            Layout::Split => {
                let term_rect = Rect { x: 0, y: 2, w: 49, h: 22 };
                let file_rect = Rect { x: 49, y: 2, w: 31, h: 22 };
                self.draw_window(term_rect, " Terminal ", self.focus == Focus::Terminal);
                self.terminal.render(&mut self.console, term_rect);
                self.draw_window(file_rect, " File Browser ", self.focus == Focus::Files);
                self.files.render(&mut self.console, file_rect);
            }
            Layout::TerminalMax => {
                let rect = Rect { x: 0, y: 2, w: 80, h: 22 };
                self.draw_window(rect, " Terminal ", true);
                self.terminal.render(&mut self.console, rect);
            }
            Layout::FilesMax => {
                let rect = Rect { x: 0, y: 2, w: 80, h: 22 };
                self.draw_window(rect, " File Browser ", true);
                self.files.render(&mut self.console, rect);
            }
        }

        self.console.set_attr(ATTR_STATUS);
        self.console.fill_at(0, SCREEN_H - 1, SCREEN_W, b' ');
        self.console.write_at(1, SCREEN_H - 1, "VMware bootable Rust OS POC - no host OS required after firmware handoff");
        self.console.set_attr(ATTR_NORMAL);
    }

    fn draw_window(&mut self, rect: Rect, title: &str, focused: bool) {
        let attr = if focused { ATTR_FOCUS } else { ATTR_INACTIVE };
        self.console.set_attr(attr);

        let x2 = rect.x + rect.w - 1;
        let y2 = rect.y + rect.h - 1;
        self.console.write_at(rect.x, rect.y, "+");
        self.console.fill_at(rect.x + 1, rect.y, rect.w.saturating_sub(2), b'-');
        self.console.write_at(x2, rect.y, "+");
        self.console.write_at(rect.x + 2, rect.y, title);

        let mut y = rect.y + 1;
        while y < y2 {
            self.console.write_at(rect.x, y, "|");
            self.console.write_at(x2, y, "|");
            y += 1;
        }

        self.console.write_at(rect.x, y2, "+");
        self.console.fill_at(rect.x + 1, y2, rect.w.saturating_sub(2), b'-');
        self.console.write_at(x2, y2, "+");
        self.console.set_attr(ATTR_NORMAL);
    }
}

#[derive(Clone, Copy)]
struct Line {
    len: usize,
    bytes: [u8; 96],
}

impl Line {
    const fn empty() -> Self {
        Self { len: 0, bytes: [0; 96] }
    }
}

struct Terminal {
    lines: [Line; 128],
    next: usize,
    count: usize,
    input: [u8; 80],
    input_len: usize,
    cwd: usize,
}

impl Terminal {
    const fn new() -> Self {
        Self {
            lines: [Line::empty(); 128],
            next: 0,
            count: 0,
            input: [0; 80],
            input_len: 0,
            cwd: 0,
        }
    }

    fn handle_key(&mut self, key: InputKey) {
        match key.unicode_char {
            8 => {
                if self.input_len > 0 {
                    self.input_len -= 1;
                }
            }
            13 => self.submit(),
            ch if ch >= 32 && ch < 127 => {
                if self.input_len < self.input.len() {
                    self.input[self.input_len] = ch as u8;
                    self.input_len += 1;
                }
            }
            _ => {}
        }
    }

    fn submit(&mut self) {
        let mut prompt = [0u8; 84];
        prompt[0] = b'>';
        prompt[1] = b' ';
        let mut i = 0;
        while i < self.input_len && i + 2 < prompt.len() {
            prompt[i + 2] = self.input[i];
            i += 1;
        }
        self.add_line(&prompt[..self.input_len + 2]);
        let mut command = [0u8; 80];
        copy_bytes(&mut command, 0, &self.input[..self.input_len]);
        let command_len = self.input_len;
        self.process_command(&command[..command_len]);
        self.input_len = 0;
    }

    fn process_command(&mut self, command: &[u8]) {
        let cmd = trim(command);
        if cmd.is_empty() {
            return;
        }

        if eq(cmd, b"help") {
            self.add_line(b"Commands: help clear ls pwd cd cat echo win about");
            self.add_line(b"Example: ls, cat README.TXT, cd docs, cat NOTES.TXT");
        } else if eq(cmd, b"clear") {
            self.next = 0;
            self.count = 0;
        } else if eq(cmd, b"ls") {
            self.cmd_ls();
        } else if eq(cmd, b"pwd") {
            self.add_line(DIRS[self.cwd].path.as_bytes());
        } else if starts_with(cmd, b"cd ") {
            self.cmd_cd(trim(&cmd[3..]));
        } else if starts_with(cmd, b"cat ") {
            self.cmd_cat(trim(&cmd[4..]));
        } else if starts_with(cmd, b"echo ") {
            self.add_line(trim(&cmd[5..]));
        } else if eq(cmd, b"win") {
            self.add_line(b"Window manager: Tab changes focus, Esc maximizes/restores.");
        } else if eq(cmd, b"about") {
            self.add_line(b"RustOS POC: a standalone Rust UEFI environment.");
        } else {
            self.add_line(b"Unknown command. Type 'help'.");
        }
    }

    fn cmd_ls(&mut self) {
        let mut found = false;
        let mut i = 0;
        while i < NODES.len() {
            let node = &NODES[i];
            if node.parent == self.cwd {
                found = true;
                let mut line = [0u8; 96];
                let name = node.name.as_bytes();
                copy_bytes(&mut line, 0, name);
                let len = name.len();
                if node.is_dir && len < line.len() {
                    line[len] = b'/';
                    self.add_line(&line[..len + 1]);
                } else {
                    self.add_line(&line[..len]);
                }
            }
            i += 1;
        }
        if !found {
            self.add_line(b"<empty>");
        }
    }

    fn cmd_cd(&mut self, name: &[u8]) {
        if eq(name, b"/") {
            self.cwd = 0;
            return;
        }
        if eq(name, b"..") {
            self.cwd = DIRS[self.cwd].parent;
            return;
        }

        if let Some(dir) = find_child_dir(self.cwd, name) {
            self.cwd = dir;
        } else {
            self.add_line(b"Directory not found.");
        }
    }

    fn cmd_cat(&mut self, name: &[u8]) {
        if let Some(content) = find_child_file(self.cwd, name) {
            self.add_wrapped(content.as_bytes());
        } else {
            self.add_line(b"File not found.");
        }
    }

    fn add_wrapped(&mut self, bytes: &[u8]) {
        let mut start = 0;
        while start < bytes.len() {
            let mut end = min(start + 90, bytes.len());
            if end < bytes.len() {
                let mut split = end;
                while split > start && bytes[split - 1] != b' ' {
                    split -= 1;
                }
                if split > start {
                    end = split;
                }
            }
            self.add_line(trim(&bytes[start..end]));
            start = end;
            while start < bytes.len() && bytes[start] == b' ' {
                start += 1;
            }
        }
    }

    fn add_line(&mut self, bytes: &[u8]) {
        let mut line = Line::empty();
        line.len = min(bytes.len(), line.bytes.len());
        copy_bytes(&mut line.bytes, 0, &bytes[..line.len]);
        self.lines[self.next] = line;
        self.next = (self.next + 1) % self.lines.len();
        if self.count < self.lines.len() {
            self.count += 1;
        }
    }

    fn render(&self, console: &mut Console, rect: Rect) {
        let inner_x = rect.x + 1;
        let inner_y = rect.y + 1;
        let inner_w = rect.w.saturating_sub(2);
        let inner_h = rect.h.saturating_sub(2);
        let log_h = inner_h.saturating_sub(2);

        console.set_attr(ATTR_NORMAL);
        let first = self.count.saturating_sub(log_h);
        let mut row = 0;
        while row < log_h {
            let idx_in_log = first + row;
            if idx_in_log < self.count {
                let ring_idx = (self.next + self.lines.len() - self.count + idx_in_log) % self.lines.len();
                let line = self.lines[ring_idx];
                console.write_bytes_at(inner_x, inner_y + row, &line.bytes[..min(line.len, inner_w)]);
            }
            row += 1;
        }

        console.set_attr(ATTR_DIM);
        console.fill_at(inner_x, inner_y + log_h, inner_w, b'-');
        console.set_attr(ATTR_NORMAL);

        let mut prompt = [0u8; 84];
        prompt[0] = b'>';
        prompt[1] = b' ';
        copy_bytes(&mut prompt, 2, &self.input[..self.input_len]);
        let len = min(self.input_len + 2, inner_w);
        console.write_bytes_at(inner_x, inner_y + log_h + 1, &prompt[..len]);
    }
}

struct FileBrowser {
    cwd: usize,
    selected: usize,
}

impl FileBrowser {
    const fn new() -> Self {
        Self { cwd: 0, selected: 0 }
    }

    fn handle_key(&mut self, key: InputKey) {
        let count = list_count(self.cwd);
        match (key.scan_code, key.unicode_char) {
            (SCAN_UP, _) => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            (SCAN_DOWN, _) => {
                if self.selected + 1 < count {
                    self.selected += 1;
                }
            }
            (_, 8) | (SCAN_LEFT, _) => {
                self.cwd = DIRS[self.cwd].parent;
                self.selected = 0;
            }
            (_, 13) | (SCAN_RIGHT, _) => {
                if let Some(node) = nth_child(self.cwd, self.selected) {
                    if node.is_dir {
                        self.cwd = node.target_dir;
                        self.selected = 0;
                    }
                }
            }
            _ => {}
        }
    }

    fn render(&self, console: &mut Console, rect: Rect) {
        let inner_x = rect.x + 1;
        let inner_y = rect.y + 1;
        let inner_w = rect.w.saturating_sub(2);
        let inner_h = rect.h.saturating_sub(2);

        console.set_attr(ATTR_DIM);
        console.write_at(inner_x, inner_y, "Path:");
        console.set_attr(ATTR_NORMAL);
        console.write_bytes_at(inner_x + 6, inner_y, DIRS[self.cwd].path.as_bytes());

        let list_y = inner_y + 2;
        let preview_y = inner_y + inner_h.saturating_sub(6);
        let mut row = 0;
        while row + list_y < preview_y && row < list_count(self.cwd) {
            if let Some(node) = nth_child(self.cwd, row) {
                if row == self.selected {
                    console.set_attr(ATTR_FOCUS);
                    console.fill_at(inner_x, list_y + row, inner_w, b' ');
                } else {
                    console.set_attr(ATTR_NORMAL);
                }

                let prefix = if node.is_dir { b"[D] " } else { b"[F] " };
                let mut line = [0u8; 72];
                copy_bytes(&mut line, 0, prefix);
                copy_bytes(&mut line, prefix.len(), node.name.as_bytes());
                console.write_bytes_at(inner_x + 1, list_y + row, &line[..min(prefix.len() + node.name.len(), inner_w.saturating_sub(1))]);
            }
            row += 1;
        }

        console.set_attr(ATTR_DIM);
        console.fill_at(inner_x, preview_y.saturating_sub(1), inner_w, b'-');
        console.write_at(inner_x, preview_y, "Preview:");
        console.set_attr(ATTR_NORMAL);

        if let Some(node) = nth_child(self.cwd, self.selected) {
            if node.is_dir {
                console.write_at(inner_x, preview_y + 1, "Directory. Press Enter/Right to open.");
            } else {
                let bytes = node.content.as_bytes();
                let mut start = 0;
                let mut line = 0;
                while line < 4 && start < bytes.len() {
                    let end = min(start + inner_w, bytes.len());
                    console.write_bytes_at(inner_x, preview_y + 1 + line, &bytes[start..end]);
                    start = end;
                    line += 1;
                }
            }
        }
    }
}

struct Dir {
    path: &'static str,
    parent: usize,
}

struct Node {
    parent: usize,
    name: &'static str,
    is_dir: bool,
    target_dir: usize,
    content: &'static str,
}

const DIRS: [Dir; 4] = [
    Dir { path: "/", parent: 0 },
    Dir { path: "/docs", parent: 0 },
    Dir { path: "/apps", parent: 0 },
    Dir { path: "/system", parent: 0 },
];

const NODES: [Node; 8] = [
    Node { parent: 0, name: "docs", is_dir: true, target_dir: 1, content: "" },
    Node { parent: 0, name: "apps", is_dir: true, target_dir: 2, content: "" },
    Node { parent: 0, name: "system", is_dir: true, target_dir: 3, content: "" },
    Node { parent: 0, name: "README.TXT", is_dir: false, target_dir: 0, content: "This is RustOS POC: a VMware-bootable Rust UEFI environment with a text desktop, terminal, file browser, and minimal window management." },
    Node { parent: 1, name: "NOTES.TXT", is_dir: false, target_dir: 0, content: "POC scope: firmware boot, no_std Rust, text UI, fixed in-memory filesystem, two apps, and keyboard driven focus/maximize window management." },
    Node { parent: 1, name: "TODO.TXT", is_dir: false, target_dir: 0, content: "Next steps: framebuffer renderer, mouse input, real block device filesystem, memory map, interrupts, scheduler, and userspace isolation." },
    Node { parent: 2, name: "TERMINAL.APP", is_dir: false, target_dir: 0, content: "Built-in terminal app. Commands are implemented in Rust and operate on the in-memory filesystem." },
    Node { parent: 3, name: "KERNEL.INFO", is_dir: false, target_dir: 0, content: "Target: x86_64-unknown-uefi. This binary runs directly under UEFI firmware and does not depend on Windows, Linux, or another host OS." },
];

fn list_count(dir: usize) -> usize {
    let mut count = 0;
    let mut i = 0;
    while i < NODES.len() {
        if NODES[i].parent == dir {
            count += 1;
        }
        i += 1;
    }
    count
}

fn nth_child(dir: usize, n: usize) -> Option<&'static Node> {
    let mut seen = 0;
    let mut i = 0;
    while i < NODES.len() {
        if NODES[i].parent == dir {
            if seen == n {
                return Some(&NODES[i]);
            }
            seen += 1;
        }
        i += 1;
    }
    None
}

fn find_child_dir(dir: usize, name: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < NODES.len() {
        let node = &NODES[i];
        if node.parent == dir && node.is_dir && eq_ignore_ascii(node.name.as_bytes(), name) {
            return Some(node.target_dir);
        }
        i += 1;
    }
    None
}

fn find_child_file(dir: usize, name: &[u8]) -> Option<&'static str> {
    let mut i = 0;
    while i < NODES.len() {
        let node = &NODES[i];
        if node.parent == dir && !node.is_dir && eq_ignore_ascii(node.name.as_bytes(), name) {
            return Some(node.content);
        }
        i += 1;
    }
    None
}

fn trim(mut bytes: &[u8]) -> &[u8] {
    while !bytes.is_empty() && bytes[0] == b' ' {
        bytes = &bytes[1..];
    }
    while !bytes.is_empty() && bytes[bytes.len() - 1] == b' ' {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn eq(a: &[u8], b: &[u8]) -> bool {
    a == b
}

fn starts_with(a: &[u8], b: &[u8]) -> bool {
    a.len() >= b.len() && &a[..b.len()] == b
}

fn eq_ignore_ascii(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if lower(a[i]) != lower(b[i]) {
            return false;
        }
        i += 1;
    }
    true
}

fn lower(b: u8) -> u8 {
    if b'A' <= b && b <= b'Z' {
        b + 32
    } else {
        b
    }
}

fn copy_bytes(dst: &mut [u8], offset: usize, src: &[u8]) {
    if offset >= dst.len() {
        return;
    }
    let mut i = 0;
    while i < src.len() && offset + i < dst.len() {
        dst[offset + i] = src[i];
        i += 1;
    }
}

fn min(a: usize, b: usize) -> usize {
    if a < b { a } else { b }
}

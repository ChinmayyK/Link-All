import SwiftUI
import AppKit

/// Notes, links and snippets shared with your own devices. They are kept on
/// each device and sent only to paired ones; edits made elsewhere show up here
/// within a couple of seconds while the window is open.
@MainActor
final class ScratchpadModel: ObservableObject {
    @Published var notes: [IpcScratchNote] = []
    @Published var problem: String?
    private var timer: Timer?
    private let ipc = LinkAllIPCClient.shared

    func start() {
        reload()
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: 2, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.reload() }
        }
    }

    func stop() {
        timer?.invalidate()
        timer = nil
    }

    func reload() {
        Task {
            do {
                let list = try await ipc.scratchpadList()
                if list != notes { notes = list }
                problem = nil
            } catch {
                problem = "Link All isn't running."
            }
        }
    }

    func add(_ text: String) { run { try await self.ipc.scratchpadSave(id: nil, text: text, pinned: false) } }
    func edit(_ note: IpcScratchNote, text: String) { run { try await self.ipc.scratchpadSave(id: note.id, text: text, pinned: note.pinned) } }
    func togglePin(_ note: IpcScratchNote) { run { try await self.ipc.scratchpadSave(id: note.id, text: note.text, pinned: !note.pinned) } }
    func delete(_ note: IpcScratchNote) { run { try await self.ipc.scratchpadDelete(id: note.id) } }
    func clear() { run { try await self.ipc.scratchpadClear() } }

    private func run(_ call: @escaping () async throws -> Void) {
        Task {
            do {
                try await call()
                problem = nil
            } catch {
                problem = error.localizedDescription
            }
            reload()
        }
    }
}

struct ScratchpadView: View {
    @StateObject private var model = ScratchpadModel()
    @State private var draft = ""
    @State private var editing: IpcScratchNote?
    @State private var confirmClear = false

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            composer
            if let problem = model.problem {
                Text(problem)
                    .font(.system(size: 12))
                    .foregroundStyle(CRTheme.accentRed)
                    .padding(.horizontal, 20)
                    .padding(.bottom, 8)
            }
            if model.notes.isEmpty {
                Spacer()
                Text("Nothing here yet. Add something and it appears on your other devices.")
                    .font(.system(size: 13))
                    .foregroundStyle(CRTheme.inkSoft)
                    .multilineTextAlignment(.center)
                    .padding(32)
                Spacer()
            } else {
                ScrollView {
                    LazyVStack(spacing: 10) {
                        ForEach(model.notes) { note in
                            NoteCard(
                                note: note,
                                onEdit: { editing = note },
                                onCopy: { copy(note.text) },
                                onPin: { model.togglePin(note) },
                                onDelete: { model.delete(note) }
                            )
                        }
                    }
                    .padding(16)
                }
            }
        }
        .frame(minWidth: 420, minHeight: 480)
        .onAppear { model.start() }
        .onDisappear { model.stop() }
        .sheet(item: $editing) { note in
            EditNoteSheet(note: note) { text in
                editing = nil
                model.edit(note, text: text)
            } onCancel: { editing = nil }
        }
        .alert("Clear all notes?", isPresented: $confirmClear) {
            Button("Cancel", role: .cancel) {}
            Button("Clear all", role: .destructive) { model.clear() }
        } message: {
            Text("This deletes every note that isn't pinned, here and on your paired devices.")
        }
    }

    private var header: some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text("Notes")
                    .font(.system(size: 16, weight: .semibold, design: .rounded))
                Text("Shared with your paired devices, nowhere else")
                    .font(.system(size: 11))
                    .foregroundStyle(CRTheme.inkSoft)
            }
            Spacer()
            if model.notes.contains(where: { !$0.pinned }) {
                Button("Clear all") { confirmClear = true }
                    .buttonStyle(.plain)
                    .foregroundStyle(CRTheme.accentBlue)
            }
        }
        .padding(16)
    }

    private var composer: some View {
        HStack(alignment: .top, spacing: 8) {
            TextField("A link, a command, a thought…", text: $draft, axis: .vertical)
                .textFieldStyle(.roundedBorder)
                .lineLimit(1...4)
                .onSubmit(submit)
            Button("Add", action: submit)
                .keyboardShortcut(.return, modifiers: .command)
                .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        }
        .padding(16)
    }

    private func submit() {
        let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        draft = ""
        model.add(text)
    }

    private func copy(_ text: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }
}

private struct NoteCard: View {
    let note: IpcScratchNote
    let onEdit: () -> Void
    let onCopy: () -> Void
    let onPin: () -> Void
    let onDelete: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            MarkdownText(text: note.text)
                .frame(maxWidth: .infinity, alignment: .leading)
            HStack(spacing: 16) {
                iconButton("doc.on.doc", "Copy", action: onCopy)
                iconButton(note.pinned ? "pin.fill" : "pin", note.pinned ? "Unpin" : "Pin", tint: note.pinned ? CRTheme.accentBlue : nil, action: onPin)
                iconButton("pencil", "Edit", action: onEdit)
                iconButton("trash", "Delete", action: onDelete)
            }
        }
        .padding(14)
        .background(CRTheme.surfaceStrong, in: RoundedRectangle(cornerRadius: 12))
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .strokeBorder(note.pinned ? CRTheme.accentBlue : CRTheme.stroke, lineWidth: 1)
        )
        .contentShape(Rectangle())
        .onTapGesture(count: 2, perform: onEdit)
    }

    private func iconButton(_ symbol: String, _ label: String, tint: Color? = nil, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: 13))
                .foregroundStyle(tint ?? CRTheme.inkSoft)
        }
        .buttonStyle(.plain)
        .help(label)
        .accessibilityLabel(label)
    }
}

private struct EditNoteSheet: View {
    let note: IpcScratchNote
    let onSave: (String) -> Void
    let onCancel: () -> Void
    @State private var text: String

    init(note: IpcScratchNote, onSave: @escaping (String) -> Void, onCancel: @escaping () -> Void) {
        self.note = note
        self.onSave = onSave
        self.onCancel = onCancel
        _text = State(initialValue: note.text)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Edit note").font(.system(size: 15, weight: .semibold))
            Text("Markdown works: # heading, **bold**, `code`, - list")
                .font(.system(size: 11))
                .foregroundStyle(CRTheme.inkSoft)
            TextEditor(text: $text)
                .font(.system(size: 13, design: .monospaced))
                .frame(minWidth: 380, minHeight: 220)
                .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(CRTheme.stroke))
            HStack {
                Spacer()
                Button("Cancel", action: onCancel).keyboardShortcut(.cancelAction)
                Button("Save") { onSave(text.trimmingCharacters(in: .whitespacesAndNewlines)) }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
    }
}

/// A small subset of Markdown, enough for notes: headings, bullets, **bold**,
/// `code` and fenced blocks. Anything else shows as written.
struct MarkdownText: View {
    let text: String

    private struct Line: Identifiable {
        let id: Int
        let text: String
        let kind: Kind
        enum Kind { case plain, heading, bullet, code }
    }

    private var lines: [Line] {
        var inFence = false
        var out: [Line] = []
        for (i, raw) in text.components(separatedBy: "\n").enumerated() {
            if raw.trimmingCharacters(in: .whitespaces).hasPrefix("```") {
                inFence.toggle()
                continue
            }
            if inFence { out.append(Line(id: i, text: raw, kind: .code)); continue }
            if let r = ["### ", "## ", "# "].first(where: { raw.hasPrefix($0) }) {
                out.append(Line(id: i, text: String(raw.dropFirst(r.count)), kind: .heading))
            } else if raw.hasPrefix("- ") || raw.hasPrefix("* ") {
                out.append(Line(id: i, text: "• " + raw.dropFirst(2), kind: .bullet))
            } else {
                out.append(Line(id: i, text: raw, kind: .plain))
            }
        }
        return out
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            ForEach(lines) { line in
                switch line.kind {
                case .heading:
                    inline(line.text).font(.system(size: 16, weight: .bold))
                case .code:
                    Text(line.text.isEmpty ? " " : line.text)
                        .font(.system(size: 12, design: .monospaced))
                        .padding(.horizontal, 6)
                        .background(CRTheme.stroke.opacity(0.6))
                default:
                    inline(line.text).font(.system(size: 13))
                }
            }
        }
        .textSelection(.enabled)
    }

    private func inline(_ s: String) -> Text {
        let options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        if let attributed = try? AttributedString(markdown: s.isEmpty ? " " : s, options: options) {
            return Text(attributed)
        }
        return Text(s)
    }
}

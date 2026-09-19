import SwiftUI
import WhisperCore

struct SnippetsPage: View {
    let application: WhisperApplication
    @State private var trigger = ""
    @State private var replacement = ""
    @State private var panelOpen = false
    @State private var editing: Snippet?
    @FocusState private var triggerFocused: Bool
    private var language: AppLanguage { application.state.settings.language }
    private var snippets: SnippetsState { application.state.snippets }
    private var trimmedTrigger: String { trigger.trimmingCharacters(in: .whitespacesAndNewlines) }
    private var duplicate: Bool { snippets.entries.contains { $0.trigger.lowercased() == trimmedTrigger.lowercased() } }
    private var triggerValid: Bool { !trimmedTrigger.isEmpty && trimmedTrigger.utf16.count <= Snippet.maximumTriggerLength && !duplicate }
    private var visibleSnippets: [Snippet] {
        let search = trimmedTrigger.lowercased()
        return snippets.entries.filter {
            panelOpen || search.isEmpty || $0.trigger.lowercased().contains(search) || $0.replacement.lowercased().contains(search)
        }
    }

    var body: some View {
        let visible = visibleSnippets
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                TextField(language.text("Add a trigger or search snippets", "添加触发词或搜索片段"), text: $trigger)
                    .textFieldStyle(.roundedBorder).focused($triggerFocused)
                    .onSubmit { if triggerValid { panelOpen = true } }
                    .accessibilityIdentifier("snippet-trigger-input")
                Button(language.text("Add", "添加")) { panelOpen = true }
                    .disabled(!triggerValid).accessibilityIdentifier("snippet-add")
            }
            if duplicate {
                Text(SnippetsFailure.duplicate.message(in: language)).font(.caption).foregroundStyle(.red)
            } else if trimmedTrigger.utf16.count > Snippet.maximumTriggerLength {
                Text(SnippetsFailure.triggerTooLong.message(in: language)).font(.caption).foregroundStyle(.red)
            }
            if panelOpen { createPanel }
            if let failure = snippets.failure {
                Label(failure.message(in: language), systemImage: "exclamationmark.triangle")
                    .foregroundStyle(.red).font(.caption).accessibilityIdentifier("snippets-error")
            } else if snippets.saved {
                Text(language.text("Snippets saved.", "片段已保存。"))
                    .foregroundStyle(.secondary).font(.caption).accessibilityIdentifier("snippets-saved")
            }
            VStack(alignment: .leading, spacing: 10) {
                if snippets.entries.isEmpty {
                    emptyState
                } else {
                    Text(language.text("Snippets", "片段")).font(.caption).fontWeight(.semibold)
                    Divider()
                    if visible.isEmpty {
                        Text(language.text("No matches. Press Enter to create a snippet.", "无匹配项。按 Enter 创建片段。"))
                            .font(.caption).foregroundStyle(.secondary).frame(maxWidth: .infinity).padding(.vertical, 24)
                    }
                    LazyVStack(spacing: 8) {
                        ForEach(visible) { snippet in
                            HStack(spacing: 10) {
                                Text(snippet.trigger).lineLimit(1)
                                Image(systemName: "arrow.right").foregroundStyle(.tertiary)
                                Text(snippet.replacement).lineLimit(1).foregroundStyle(.secondary)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                Button { application.send(.dismissSnippetsMessage); editing = snippet } label: { Image(systemName: "pencil") }
                                    .accessibilityLabel(language.text("Edit \(snippet.trigger)", "编辑 \(snippet.trigger)"))
                                Button { application.send(.deleteSnippet(snippet.id)) } label: { Image(systemName: "xmark") }
                                    .accessibilityLabel(language.text("Remove \(snippet.trigger)", "删除 \(snippet.trigger)"))
                            }.buttonStyle(.plain).frame(minHeight: 28)
                            if snippet.id != visible.last?.id { Divider().opacity(0.5) }
                        }
                    }
                }
            }
            .padding(16).background(.primary.opacity(0.025))
            .clipShape(RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(.primary.opacity(0.12)))
        }
        .onAppear { application.send(.refreshSnippets) }
        .sheet(item: $editing) { SnippetEditor(application: application, snippet: $0) }
    }

    private var createPanel: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(language.text("Text to insert when you say the trigger", "说出触发词时插入的文本"))
                .font(.caption).foregroundStyle(.secondary)
            TextEditor(text: $replacement).frame(height: 90)
                .accessibilityLabel(language.text("Replacement", "替换文本"))
                .accessibilityIdentifier("snippet-replacement-input")
                .onExitCommand(perform: closePanel)
            HStack {
                Text("⌘ ↩").font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button(language.text("Cancel", "取消"), action: closePanel)
                Button(language.text("Create snippet", "创建片段")) {
                    application.send(.saveSnippet(trigger: trigger, replacement: replacement))
                    if snippets.saved { trigger = ""; closePanel() }
                }
                .keyboardShortcut(.return, modifiers: .command)
                .disabled(!triggerValid || replacement.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                .buttonStyle(.borderedProminent).accessibilityIdentifier("snippet-create")
            }
        }.padding(12).overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(.tint.opacity(0.4)))
    }

    private var emptyState: some View {
        HStack(alignment: .top, spacing: 24) {
            VStack(alignment: .leading, spacing: 10) {
                Text(language.text("The stuff you shouldn't have to say twice", "同样的话，只说一次"))
                    .fontWeight(.semibold)
                Text(language.text("Speak a trigger and Whisper inserts your saved text: URLs, intros, sign-offs, or prompts.", "说出触发词，Whisper 会插入保存的文本：链接、开场白、签名或提示词。"))
                    .font(.caption).foregroundStyle(.secondary)
                Button(language.text("New snippet", "新建片段")) { triggerFocused = true }.buttonStyle(.borderedProminent)
            }.frame(maxWidth: .infinity, alignment: .leading)
            VStack(alignment: .leading, spacing: 12) {
                example(language.text("My LinkedIn", "我的 LinkedIn"), "linkedin.com/in/you")
                example(language.text("Rewrite prompt", "改写提示词"), language.text("Rewrite this to be more concise and professional", "把这段话改写得更简洁、更专业"))
                example(language.text("Intro email", "开场邮件"), language.text("Hey, would love to find some time to chat later this week…", "你好，希望本周能找个时间聊聊…"))
                example(language.text("Sign off", "签名"), language.text("Best, Alex · alex@example.com", "祝好，Alex · alex@example.com"))
            }.frame(maxWidth: .infinity, alignment: .leading)
        }.padding(.vertical, 16)
    }

    private func example(_ trigger: String, _ text: String) -> some View {
        HStack(alignment: .top, spacing: 6) {
            Label(trigger, systemImage: "mic").foregroundStyle(.tint)
            Image(systemName: "arrow.right").foregroundStyle(.tertiary)
            Text(text).foregroundStyle(.secondary)
        }.font(.caption)
    }

    private func closePanel() { panelOpen = false; replacement = ""; triggerFocused = true }
}

private struct SnippetEditor: View {
    let application: WhisperApplication
    let snippet: Snippet
    @State private var trigger: String
    @State private var replacement: String
    @Environment(\.dismiss) private var dismiss
    private var language: AppLanguage { application.state.settings.language }
    private var duplicate: Bool {
        application.state.snippets.entries.contains {
            $0.id != snippet.id && $0.trigger.lowercased() == trigger.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        }
    }
    private var valid: Bool {
        let trimmed = trigger.trimmingCharacters(in: .whitespacesAndNewlines)
        return !trimmed.isEmpty && trimmed.utf16.count <= Snippet.maximumTriggerLength
            && !replacement.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !duplicate
    }

    init(application: WhisperApplication, snippet: Snippet) {
        self.application = application
        self.snippet = snippet
        self._trigger = State(initialValue: snippet.trigger)
        self._replacement = State(initialValue: snippet.replacement)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(language.text("Edit snippet", "编辑片段")).font(.headline)
            Text(language.text("When you speak the trigger, it's replaced with your saved text.", "当你说出触发词时，它会被替换为保存的文本。"))
                .font(.caption).foregroundStyle(.secondary)
            Text(language.text("Trigger", "触发词")).fontWeight(.semibold)
            TextField(language.text("e.g. cal link", "例如：我的日程链接"), text: $trigger).textFieldStyle(.roundedBorder)
                .accessibilityIdentifier("snippet-edit-trigger")
            Text(language.text("Replacement", "替换文本")).fontWeight(.semibold)
            TextEditor(text: $replacement).frame(height: 120).accessibilityIdentifier("snippet-edit-replacement")
            if duplicate {
                Text(SnippetsFailure.duplicate.message(in: language)).font(.caption).foregroundStyle(.red)
            } else if trigger.trimmingCharacters(in: .whitespacesAndNewlines).utf16.count > Snippet.maximumTriggerLength {
                Text(SnippetsFailure.triggerTooLong.message(in: language)).font(.caption).foregroundStyle(.red)
            } else if let failure = application.state.snippets.failure {
                Text(failure.message(in: language)).font(.caption).foregroundStyle(.red)
            }
            HStack {
                Spacer()
                Button(language.text("Cancel", "取消")) { dismiss() }.keyboardShortcut(.cancelAction)
                Button(language.text("Save", "保存")) {
                    application.send(.saveSnippet(trigger: trigger, replacement: replacement, editingID: snippet.id))
                    if application.state.snippets.saved { dismiss() }
                }.disabled(!valid).buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
            }
        }.padding(24).frame(width: 430).font(.custom("JetBrainsMono-Regular", size: 13))
    }
}

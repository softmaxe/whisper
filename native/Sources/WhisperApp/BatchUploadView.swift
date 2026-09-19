import SwiftUI
import UniformTypeIdentifiers
import WhisperCore

struct BatchUploadView: View {
    let application: WhisperApplication
    let onAddFiles: () -> Void
    let onOpenHistory: () -> Void
    let onOpenSettings: () -> Void
    @State private var dragOver = false
    private var language: AppLanguage { application.state.settings.language }
    private var batch: BatchUploadState { application.state.batchUpload }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text(language.text("\(batch.completedCount) of \(batch.items.count) completed", "已完成 \(batch.completedCount) / \(batch.items.count)"))
                    .font(.headline)
                Spacer()
                Button(language.text("Add files", "添加文件"), action: onAddFiles)
                    .accessibilityIdentifier("batch-upload-add")
                if !batch.isProcessing {
                    Button(language.text("Clear queue", "清空队列")) { application.send(.clearUploadBatch) }
                        .accessibilityIdentifier("batch-upload-clear")
                }
            }
            if batch.failedCount > 0 || batch.cancelledCount > 0 {
                Text(language.text("\(batch.failedCount) failed · \(batch.cancelledCount) cancelled", "\(batch.failedCount) 个失败 · \(batch.cancelledCount) 个已取消"))
                    .font(.caption).foregroundStyle(.secondary)
            }
            ProgressView(value: batch.progress)
                .accessibilityLabel(language.text("Files processed", "文件处理进度"))
            if let failure = batch.failure {
                Label(failure.message(in: language), systemImage: "exclamationmark.triangle")
                    .foregroundStyle(.red).font(.caption)
                Button(language.text("Speech-to-Text settings", "语音转文字设置"), action: onOpenSettings)
            }
            ScrollView {
                LazyVStack(spacing: 8) {
                    ForEach(batch.items) { item in
                        BatchUploadRow(application: application, item: item, onOpenHistory: onOpenHistory)
                    }
                }
            }
            .frame(minHeight: 100, maxHeight: 380)
            if batch.isProcessing {
                Button(language.text("Cancel remaining", "取消剩余任务")) { application.send(.cancelUploadBatch) }
                    .keyboardShortcut(.escape, modifiers: []).accessibilityIdentifier("batch-upload-cancel")
                    .frame(maxWidth: .infinity)
            } else if batch.hasQueued {
                Button(language.text("Transcribe queued files", "转录队列中的文件")) { application.send(.startUploadBatch) }
                    .buttonStyle(.borderedProminent).accessibilityIdentifier("batch-upload-start")
                    .frame(maxWidth: .infinity)
            }
            if batch.items.contains(where: { !$0.text.isEmpty && $0.historyID == nil && !$0.historySaveFailed }) {
                Text(language.text("Unsaved results remain available to copy here.", "未保存的结果仍可在此复制。"))
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .padding(18)
        .background(dragOver ? Color.accentColor.opacity(0.06) : Color.primary.opacity(0.025), in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(dragOver ? Color.accentColor : Color.primary.opacity(0.12)))
        .dropDestination(for: URL.self) { urls, _ in
            application.send(.chooseUploadFiles(urls))
            return !urls.isEmpty
        } isTargeted: { dragOver = $0 }
    }
}

private struct BatchUploadRow: View {
    let application: WhisperApplication
    let item: BatchUploadItem
    let onOpenHistory: () -> Void
    private var language: AppLanguage { application.state.settings.language }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 9) {
                if item.status.isActive { ProgressView().controlSize(.small) }
                else { Image(systemName: icon).foregroundStyle(item.status == .failed ? .red : .secondary) }
                VStack(alignment: .leading, spacing: 4) {
                    Text(item.name).lineLimit(2).frame(maxWidth: .infinity, alignment: .leading)
                    Text(status).font(.caption).foregroundStyle(.secondary)
                }
                if item.status == .queued {
                    Button { application.send(.removeUploadBatchItem(item.id)) } label: { Image(systemName: "trash") }
                        .accessibilityLabel(language.text("Remove from queue", "从队列移除"))
                }
            }
            if let failure = item.failure {
                Text(failure.message(in: language)).font(.caption).foregroundStyle(.red)
            }
            if item.historySaveFailed {
                Text(language.text("History could not be saved. The transcript is still available.", "无法保存历史记录，转录结果仍可使用。"))
                    .font(.caption).foregroundStyle(.orange)
            }
            if !item.text.isEmpty {
                DisclosureGroup(language.text("Transcript", "转录结果")) {
                    Text(item.text).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading).padding(.top, 8)
                }
                HStack {
                    Button(language.text(item.resultCopied ? "Copied" : "Copy", item.resultCopied ? "已复制" : "复制")) {
                        application.send(.copyUploadBatchItem(item.id))
                    }
                    if item.historyID != nil { Button(language.text("Open History", "查看历史记录"), action: onOpenHistory) }
                    Spacer()
                }
                .buttonStyle(.borderless)
            }
            if item.copyFailed {
                Text(language.text("Copy failed. Select the transcript and copy it manually.", "复制失败。请选中转录文字并手动复制。"))
                    .font(.caption).foregroundStyle(.orange)
            }
        }
        .padding(12).background(.primary.opacity(0.025), in: RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(.primary.opacity(0.08)))
        .accessibilityIdentifier("batch-upload-item-" + item.id.uuidString)
    }

    private var icon: String {
        switch item.status {
        case .done: "checkmark.circle"
        case .failed: "exclamationmark.circle"
        case .cancelled: "xmark.circle"
        default: "clock"
        }
    }

    private var status: String {
        switch item.status {
        case .queued: language.text("Queued", "等待中")
        case .preparing: language.text("Preparing audio…", "正在准备音频…")
        case .transcribing: language.text("Transcribing…", "正在转录…")
        case .saving: language.text("Saving history…", "正在保存历史记录…")
        case .done: language.text("Complete", "已完成")
        case .failed: language.text("Failed", "失败")
        case .cancelled: language.text("Cancelled", "已取消")
        }
    }
}

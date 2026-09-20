import AppKit
import SwiftUI
import UniformTypeIdentifiers
import WhisperCore

struct UploadView: View {
    let application: WhisperApplication
    var onOpenSettings: () -> Void = {}
    var onOpenHistory: () -> Void = {}
    @State private var dragOver = false
    private var language: AppLanguage { application.state.settings.language }
    private var upload: UploadState { application.state.upload }
    private var batch: BatchUploadState { application.state.batchUpload }
    private var configured: Bool { !application.state.settings.asr.serverURL.isEmpty && !application.state.settings.asr.model.isEmpty }

    var body: some View {
        VStack(alignment: .leading, spacing: 22) {
            VStack(alignment: .leading, spacing: 8) {
                Text(language.text("Upload audio", "上传音频")).font(.title2).fontWeight(.semibold)
                Text(language.text("Transcribe audio or video files with your Speech-to-Text server.", "使用您的语音转文字服务器转录音频或视频文件。"))
                    .foregroundStyle(.secondary)
            }
            if !batch.items.isEmpty {
                BatchUploadView(application: application, onAddFiles: browse, onOpenHistory: onOpenHistory, onOpenSettings: onOpenSettings)
            } else if upload.phase == .idle {
                VStack(spacing: 16) {
                    Image(systemName: "arrow.up.doc").font(.system(size: 34)).foregroundStyle(.secondary)
                        .accessibilityHidden(true)
                    Text(language.text("Drop audio or video files here", "将音频或视频文件拖到此处"))
                    Button(language.text("Browse files", "浏览文件"), action: browse)
                        .buttonStyle(.borderedProminent).accessibilityIdentifier("upload-browse")
                    Text(language.text("MP3, WAV, M4A, MP4, MOV and other supported formats", "支持 MP3、WAV、M4A、MP4、MOV 等格式"))
                        .font(.caption).foregroundStyle(.secondary)
                    if !configured {
                        Button(language.text("Configure Speech-to-Text", "配置语音转文字"), action: onOpenSettings)
                    }
                }
                .frame(maxWidth: .infinity).padding(36)
                .background(dragOver ? Color.accentColor.opacity(0.08) : Color.primary.opacity(0.025), in: RoundedRectangle(cornerRadius: 14))
                .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(dragOver ? Color.accentColor : Color.primary.opacity(0.15), style: StrokeStyle(lineWidth: 1, dash: [5])))
                .dropDestination(for: URL.self) { urls, _ in
                    application.send(.chooseUploadFiles(urls))
                    return !urls.isEmpty
                } isTargeted: { dragOver = $0 }
            } else {
                VStack(alignment: .leading, spacing: 16) {
                    HStack(spacing: 12) {
                        Image(systemName: "waveform").font(.title2).foregroundStyle(.secondary)
                            .accessibilityHidden(true)
                        VStack(alignment: .leading, spacing: 4) {
                            Text(upload.fileName).lineLimit(2).textSelection(.enabled)
                            if let size = upload.sourceSize { Text(ByteCountFormatter.string(fromByteCount: size, countStyle: .file)).font(.caption).foregroundStyle(.secondary) }
                        }
                        Spacer()
                        if !upload.phase.isActive {
                            Button { application.send(.resetUpload) } label: { Image(systemName: "xmark") }
                                .accessibilityLabel(language.text("Choose another file", "选择其他文件"))
                        }
                    }
                    if upload.phase.isActive {
                        HStack {
                            ProgressView().controlSize(.small)
                            Text(upload.phase == .preparing ? language.text("Preparing audio…", "正在准备音频…") : language.text("Transcribing…", "正在转录…"))
                            Spacer()
                            Button(language.text("Cancel", "取消")) { application.send(.cancelUpload) }
                                .keyboardShortcut(.escape, modifiers: []).accessibilityIdentifier("upload-cancel")
                        }
                        .accessibilityElement(children: .contain).accessibilityIdentifier("upload-progress")
                    } else if upload.phase == .selected {
                        HStack {
                            Text(application.state.settings.asr.model).font(.caption).foregroundStyle(.secondary)
                            Spacer()
                            Button(language.text("Transcribe", "开始转录")) { application.send(.startUpload) }
                                .buttonStyle(.borderedProminent).accessibilityIdentifier("upload-start")
                        }
                    }
                    if let failure = upload.failure {
                        Label(failure.message(in: language), systemImage: "exclamationmark.triangle")
                            .foregroundStyle(.red).font(.callout).accessibilityIdentifier("upload-error")
                        HStack {
                            Button(language.text("Try again", "重试")) { application.send(.startUpload) }
                            Button(language.text("Speech-to-Text settings", "语音转文字设置"), action: onOpenSettings)
                        }
                    }
                }
                .padding(20).background(.primary.opacity(0.025), in: RoundedRectangle(cornerRadius: 12))
                .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(.primary.opacity(0.12)))
            }
            if !batch.skippedNames.isEmpty {
                Text(language.text("Unsupported files were skipped: ", "已跳过不支持的文件：") + batch.skippedNames.joined(separator: ", "))
                    .font(.caption).foregroundStyle(.secondary)
            }
            if !upload.text.isEmpty && batch.items.isEmpty {
                VStack(alignment: .leading, spacing: 14) {
                    Label(language.text("Transcription complete", "转录完成"), systemImage: "checkmark.circle")
                        .font(.headline)
                    ScrollView {
                        Text(upload.text).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading).padding(14)
                    }
                    .frame(minHeight: 120, maxHeight: 360)
                    .background(.primary.opacity(0.025), in: RoundedRectangle(cornerRadius: 10))
                    .accessibilityIdentifier("upload-result")
                    if upload.historySaveFailed {
                        Label(language.text("History could not be saved. You can still copy the transcript.", "无法保存历史记录，仍可复制转录结果。"), systemImage: "exclamationmark.triangle")
                            .font(.caption).foregroundStyle(.orange)
                    } else if upload.isSavingHistory {
                        Text(language.text("Saving history…", "正在保存历史记录…")).font(.caption).foregroundStyle(.secondary)
                    }
                    if upload.copyFailed {
                        Text(language.text("Copy failed. Select the transcript and copy it manually.", "复制失败。请选中转录文字并手动复制。"))
                            .font(.caption).foregroundStyle(.orange)
                    }
                    HStack {
                        Button(language.text(upload.resultCopied ? "Copied" : "Copy transcript", upload.resultCopied ? "已复制" : "复制转录")) { application.send(.copyUploadResult) }
                            .buttonStyle(.borderedProminent).accessibilityIdentifier("upload-copy")
                        if upload.historyID != nil { Button(language.text("Open History", "查看历史记录"), action: onOpenHistory) }
                        Spacer()
                        Button(language.text("Transcribe another file", "转录其他文件")) { application.send(.resetUpload) }
                    }
                }
            }
        }
    }

    private func browse() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = true
        panel.allowedContentTypes = UploadFormats.extensions.compactMap { UTType(filenameExtension: $0) }
        panel.begin { result in
            guard result == .OK else { return }
            application.send(.chooseUploadFiles(panel.urls))
        }
    }
}

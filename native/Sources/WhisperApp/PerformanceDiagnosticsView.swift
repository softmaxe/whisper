import AppKit
import SwiftUI
import WhisperCore

struct PerformanceDiagnosticsView: View {
    let application: WhisperApplication
    private var language: AppLanguage { application.state.settings.language }
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(language.text("Performance diagnostics", "性能诊断")).font(.headline)
            Text(language.text("Off by default. Save numeric request timings to a private local file. Audio, text, device names and server details are excluded.", "默认关闭。将请求的数值计时保存到私有本地文件，不包含音频、文字、设备名称或服务器详情。"))
                .font(.caption).foregroundStyle(.secondary)
            if application.state.diagnostics.enabled {
                Text(language.text("Recording diagnostics for new Dictations.", "正在为新的听写记录诊断数据。"))
                HStack {
                    Button(language.text("Flush to file", "写入文件")) { Task { await application.flushDiagnostics() } }
                    Button(language.text("Stop diagnostics", "停止诊断")) { application.send(.setDiagnosticsOutput(nil)) }
                }
            } else {
                Button(language.text("Choose diagnostics file…", "选择诊断文件…")) {
                    let panel = NSSavePanel()
                    panel.nameFieldStringValue = "native-timing.jsonl"
                    if panel.runModal() == .OK, let destination = panel.url { application.send(.setDiagnosticsOutput(destination)) }
                }
            }
            if application.state.diagnostics.writeFailed {
                Text(language.text("Diagnostics could not be written. Choose a private file created by Whisper, or a new filename. Dictation is unaffected.", "无法写入诊断。请选择 Whisper 创建的私有文件或新的文件名。听写功能不受影响。"))
                    .font(.caption).foregroundStyle(.red)
            }
            if application.state.diagnostics.droppedRecords > 0 {
                Text(language.text("Some diagnostic records were dropped. This collection is incomplete.", "部分诊断记录未写入，此次采集不完整。"))
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .padding(18).background(.primary.opacity(0.025), in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(.primary.opacity(0.12)))
    }
}

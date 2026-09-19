import Foundation

public enum ChineseScriptPreference: String, Codable, CaseIterable, Sendable {
    case asTranscribed = "as-transcribed"
    case simplified, traditional

    public init(from decoder: any Decoder) throws {
        let value = try decoder.singleValueContainer().decode(String.self)
        self = Self(rawValue: value) ?? .asTranscribed
    }

    public func title(in language: AppLanguage) -> String {
        switch self {
        case .asTranscribed: language.text("Keep as transcribed", "保持转录原样")
        case .simplified: language.text("Simplified (简体)", "简体")
        case .traditional: language.text("Traditional (繁體)", "繁体")
        }
    }
}

public struct TranscriptionLanguage: Decodable, Identifiable, Sendable {
    public let code: String
    public let label: String
    public var id: String { code }

    public static let all: [Self] = {
        struct Registry: Decodable { let languages: [TranscriptionLanguage] }
        guard let url = Bundle.module.url(forResource: "languageRegistry", withExtension: "json", subdirectory: "Resources"),
              let data = try? Data(contentsOf: url),
              let registry = try? JSONDecoder().decode(Registry.self, from: data) else {
            preconditionFailure("The bundled transcription language registry is unavailable.")
        }
        return registry.languages
    }()

    public func title(in language: AppLanguage) -> String {
        if code == "auto" { return language.text("Auto-detect", "自动检测") }
        if code == "zh-CN" { return language.text(label, "中文（简体）") }
        if code == "zh-TW" { return language.text(label, "中文（繁体）") }
        return label
    }
}

public struct TranscriptionPreferences: Codable, Equatable, Sendable {
    public var preferredLanguage: String
    public var chineseScriptPreference: ChineseScriptPreference

    public init(preferredLanguage: String = "auto", chineseScriptPreference: ChineseScriptPreference = .asTranscribed) {
        self.preferredLanguage = preferredLanguage.isEmpty ? "auto" : preferredLanguage
        self.chineseScriptPreference = chineseScriptPreference
    }

    public init(from decoder: any Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        self.init(
            preferredLanguage: try values.decodeIfPresent(String.self, forKey: .preferredLanguage) ?? "auto",
            chineseScriptPreference: try values.decodeIfPresent(ChineseScriptPreference.self, forKey: .chineseScriptPreference) ?? .asTranscribed
        )
    }

    public func requestOptions(dictionaryPrompt: String? = nil) -> TranscriptionOptions {
        let base = preferredLanguage == "auto" ? nil : preferredLanguage.split(separator: "-").first.map(String.init)
        let bias: String? = switch preferredLanguage {
        case "zh-CN": "以下是简体中文。语言、学习、软件、网络。"
        case "zh-TW": "以下是繁體中文。語言、學習、軟體、網路。"
        default: nil
        }
        let hints = dictionaryPrompt?.trimmingCharacters(in: .whitespacesAndNewlines)
        let prompt = [bias, hints].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " ")
        return TranscriptionOptions(language: base, prompt: prompt.isEmpty ? nil : prompt)
    }

    func target(for text: String) -> ChineseScriptPreference? {
        if preferredLanguage == "zh-CN" { return .simplified }
        if preferredLanguage == "zh-TW" { return .traditional }
        guard preferredLanguage == "auto", chineseScriptPreference != .asTranscribed else { return nil }
        let scalars = text.unicodeScalars
        guard !scalars.contains(where: { scalar in
            let value = scalar.value
            return (0x3040...0x30ff).contains(value) || (0x31f0...0x31ff).contains(value)
                || (0xff66...0xff9f).contains(value) || (0x1100...0x11ff).contains(value)
                || (0x3130...0x318f).contains(value) || (0xac00...0xd7af).contains(value)
        }), scalars.contains(where: { Self.chineseSignals.contains($0.value) }) else { return nil }
        return chineseScriptPreference
    }

    private static let chineseSignals = Set(
        ("这们吗简软网语汉习说发东车门问间书见长爱据实认让给还过边达选进运远违连迟适应际标亲亿优仅从众气请谢听读卖产业电备复历压类总处线证验权转导报记试计机传云丰动务场图库录页价关规办觉坏变删乐广设"
        + "這們嗎體說發據實讓邊遲應氣條聽讀寫賣兒經廣樂觀號國學會產歷壓總處證驗權轉傳豐圖錄價關辦覺壞變刪")
            .unicodeScalars.map(\.value)
    )
}

extension WhisperApplication {
    func setTranscriptionLanguage(_ code: String) {
        guard TranscriptionLanguage.all.contains(where: { $0.code == code }) else { return }
        var settings = state.settings
        settings.transcription.preferredLanguage = code
        persist(settings)
    }

    func setChineseScriptPreference(_ preference: ChineseScriptPreference) {
        var settings = state.settings
        settings.transcription.chineseScriptPreference = preference
        persist(settings)
    }

    // Cleanup hands its completed or fallback text here. Snippets follow conversion at integration.
    func finishDictationText(rawText: String, text: String, requestID: UUID, preferences: TranscriptionPreferences) async {
        guard isCurrentDictation(requestID) else { return }
        var result = text
        markDictationStage("textConversion", requestID: requestID)
        do {
            result = try await ChineseScriptConverter.shared.convert(text, preferences: preferences)
        } catch is CancellationError {
            return
        } catch {
            guard isCurrentDictation(requestID) else { return }
            state.dictation.chineseConversionFailed = true
        }
        guard isCurrentDictation(requestID), !Task.isCancelled else { return }
        completeDictation(rawText: rawText, text: result, requestID: requestID)
    }
}

import SwiftUI
import WhisperCore

struct InsightsView: View {
    let application: WhisperApplication
    private var language: AppLanguage { application.state.settings.language }
    private var state: InsightsState { application.state.insights }
    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            HStack {
                Text(language.text("Your Usage", "你的使用情况"))
                    .font(.caption).padding(.horizontal, 12).padding(.vertical, 7)
                    .background(.primary.opacity(0.06), in: RoundedRectangle(cornerRadius: 7))
                Spacer()
                Label(language.text("On this device", "仅在本设备"), systemImage: "externaldrive")
                    .font(.caption).foregroundStyle(.secondary)
            }
            if let failure = state.failure {
                VStack(spacing: 12) {
                    Text(failure.message(in: language)).foregroundStyle(.secondary)
                    Button(language.text("Retry", "重试")) { application.send(.loadInsights) }
                }.frame(maxWidth: .infinity).padding(40)
            } else if let summary = state.summary {
                if !application.state.settings.history.enabled {
                    Label(language.text("Data retention is off. New dictations do not count toward Insights.", "数据保留已关闭。新的听写不会计入使用统计。"), systemImage: "nosign")
                        .font(.caption).foregroundStyle(.orange).padding(14)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(.orange.opacity(0.06), in: RoundedRectangle(cornerRadius: 8))
                }
                if summary.totalDictations == 0 {
                    VStack(spacing: 12) {
                        Image(systemName: "chart.bar").font(.system(size: 38)).foregroundStyle(.secondary)
                        Text(language.text("No dictations yet", "还没有听写记录")).fontWeight(.medium)
                        Text(language.text("Start dictating and your words, streaks, and daily activity will show up here.", "开始听写后，你的字数、连续记录和每日活动会显示在这里。"))
                            .font(.caption).foregroundStyle(.secondary)
                    }.frame(maxWidth: .infinity).padding(.vertical, 56)
                } else {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 148), spacing: 12)], spacing: 12) {
                        metric("mic", language.text("Words spoken", "已听写字数"), number(summary.totalWords), language.text("All time", "全部时间"))
                        metric("gauge.with.dots.needle.33percent", language.text("Words per minute", "每分钟字数"), summary.averageWpm.map(number) ?? "—",
                               language.text("Based on \(summary.wpmCoveragePercent)% of measured words", "基于 \(summary.wpmCoveragePercent)% 已测量的字数"))
                        metric("chart.bar", language.text("Dictations", "听写次数"), number(summary.totalDictations), language.text("All time", "全部时间"))
                        metric("flame", language.text("Current streak", "当前连续天数"), days(summary.currentStreakDays),
                               language.text("Longest: \(days(summary.longestStreakDays))", "最长：\(days(summary.longestStreakDays))"))
                    }
                    VStack(alignment: .leading, spacing: 14) {
                        Text(language.text("Speaking activity", "听写活跃度")).font(.headline)
                        InsightsHeatmap(days: summary.activity, language: language)
                    }.padding(18).background(.primary.opacity(0.025), in: RoundedRectangle(cornerRadius: 14))
                        .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(.primary.opacity(0.12)))
                }
            } else {
                ProgressView(language.text("Loading…", "正在加载…")).frame(maxWidth: .infinity).padding(32)
            }
            Spacer(minLength: 16)
            Text(language.text("These insights are calculated on this device to protect your privacy.", "这些分析数据会在此设备上计算，以保护你的隐私。"))
                .font(.caption).foregroundStyle(.secondary).frame(maxWidth: .infinity)
        }
        .accessibilityIdentifier("insights-page")
        .onAppear { application.send(.loadInsights) }
    }
    private func number(_ value: Int) -> String {
        value.formatted(.number.notation(.compactName).precision(.fractionLength(0...1)).locale(Locale(identifier: language.rawValue)))
    }
    private func days(_ value: Int) -> String { language.text("\(value) \(value == 1 ? "day" : "days")", "\(value) 天") }
    private func metric(_ icon: String, _ title: String, _ value: String, _ detail: String) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Label(title, systemImage: icon).font(.caption).foregroundStyle(.secondary)
            Text(value).font(.custom("JetBrainsMono-SemiBold", size: 28))
            Text(detail).font(.system(size: 11)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }.frame(maxWidth: .infinity, minHeight: 110, alignment: .topLeading).padding(16)
            .background(.primary.opacity(0.025), in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(.primary.opacity(0.12)))
    }
}

private struct InsightsHeatmap: View {
    let days: [InsightsActivityDay]
    let language: AppLanguage
    private var weeks: [[InsightsActivityDay?]] {
        guard let first = days.first, let date = parsed(first.date) else { return [] }
        var cells = [InsightsActivityDay?](repeating: nil, count: (calendar.component(.weekday, from: date) + 5) % 7)
        cells += days.map(Optional.some)
        while !cells.count.isMultiple(of: 7) { cells.append(nil) }
        return stride(from: 0, to: cells.count, by: 7).map { Array(cells[$0..<($0 + 7)]) }
    }
    var body: some View {
        let weeks = weeks
        let maximum = max(1, days.map(\.words).max() ?? 1)
        ScrollView(.horizontal) {
            HStack(alignment: .top, spacing: 12) {
                VStack(spacing: 2) {
                    Color.clear.frame(height: 18)
                    ForEach(0..<7) { index in
                        Text(weekday(index)).font(.system(size: 10)).foregroundStyle(.secondary).frame(height: 24)
                    }
                }.accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 10) {
                    HStack(alignment: .top, spacing: 8) {
                        ForEach(weeks.indices, id: \.self) { index in
                            VStack(spacing: 2) {
                                Text(monthLabel(weeks[index], first: index == 0)).font(.system(size: 10))
                                    .foregroundStyle(.secondary).frame(width: 24, height: 18).accessibilityHidden(true)
                                ForEach(0..<7) { row in
                                    if let day = weeks[index][row] {
                                        RoundedRectangle(cornerRadius: 3)
                                            .fill(day.words == 0 ? Color.primary.opacity(0.06) : Color.accentColor.opacity(intensity(day.words, maximum)))
                                            .frame(width: 24, height: 24)
                                            .overlay(RoundedRectangle(cornerRadius: 3).strokeBorder(day.date == days.last?.date ? Color.accentColor : .clear))
                                            .help(tooltip(day)).accessibilityLabel(tooltip(day))
                                    } else { Color.clear.frame(width: 24, height: 24).accessibilityHidden(true) }
                                }
                            }
                        }
                    }
                    HStack(spacing: 7) {
                        Text(language.text("Less", "较少"))
                        ForEach(1..<5) { level in RoundedRectangle(cornerRadius: 2).fill(Color.accentColor.opacity(Double(level) / 4)).frame(width: 12, height: 12) }
                        Text(language.text("More", "较多"))
                    }.font(.system(size: 10)).foregroundStyle(.secondary).accessibilityHidden(true)
                }
            }.padding(3)
        }.focusable().accessibilityElement(children: .contain)
            .accessibilityLabel(language.text("6-month speaking activity heatmap", "6 个月听写活跃度热力图"))
    }
    private var calendar: Calendar { var value = Calendar(identifier: .gregorian); value.timeZone = TimeZone(secondsFromGMT: 0)!; return value }
    private func parsed(_ key: String) -> Date? {
        let parts = key.split(separator: "-").compactMap { Int($0) }
        guard parts.count == 3 else { return nil }
        return calendar.date(from: DateComponents(year: parts[0], month: parts[1], day: parts[2]))
    }
    private func weekday(_ index: Int) -> String {
        language == .simplifiedChinese ? ["周一", "周二", "周三", "周四", "周五", "周六", "周日"][index] : ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][index]
    }
    private func monthLabel(_ week: [InsightsActivityDay?], first: Bool) -> String {
        guard let day = week.compactMap({ $0 }).first(where: { $0.date.hasSuffix("-01") }) ?? (first ? week.compactMap({ $0 }).first : nil),
              let date = parsed(day.date) else { return "" }
        return date.formatted(.dateTime.month(.abbreviated).locale(Locale(identifier: language.rawValue)))
    }
    private func intensity(_ words: Int, _ maximum: Int) -> Double { [0, 0.25, 0.45, 0.7, 1][max(1, min(4, Int(ceil(Double(words) / Double(maximum) * 4))))] }
    private func tooltip(_ day: InsightsActivityDay) -> String {
        language.text("\(day.date): \(day.words) \(day.words == 1 ? "word" : "words")", "\(day.date)：\(day.words) 字")
    }
}

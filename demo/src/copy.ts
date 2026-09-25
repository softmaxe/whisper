// On-screen words for each language. App labels match src/locales; the
// dictated sentences are sample data written for the demo.

export interface Token {
  text: string;
  /** Spoken filler that text cleanup removes. */
  filler?: boolean;
}

export interface Copy {
  lang: "en" | "zh-CN";
  font: string;
  /** Joins spoken tokens: a space in English, nothing in Chinese. */
  gap: string;
  slogan: string;
  subtitle: string;
  key: { doubleTap: string; hold: string };
  clock: Record<"morning" | "chat" | "snippet" | "hold" | "upload" | "night", string>;
  menuClock: Record<"morning" | "chat" | "snippet" | "hold" | "upload" | "night", string>;
  caption: {
    dictate: string;
    cleanup: string;
    learn: string;
    snippet: string;
    hold: string;
    upload: string;
    history: string;
    insights: string;
    servers: string;
    serversNote: string;
  };
  saidLabel: string;
  cleanedLabel: string;
  mail: {
    app: string;
    to: string;
    toName: string;
    subject: string;
    subjectText: string;
    greeting: string;
    spoken: Token[];
    cleaned: string;
  };
  chat: {
    app: string;
    channel: string;
    channels: string[];
    teammate: string;
    earlier: { name: string; time: string; text: string }[];
    question: string;
    you: string;
    heard: string;
    wrong: string;
    right: string;
    toast: string;
    placeholder: string;
  };
  snippet: {
    app: string;
    contact: string;
    earlier: { mine: boolean; text: string }[];
    incoming: string;
    said: string;
    trigger: string;
    expansion: string;
    before: string;
  };
  hold: { file: string; code: string[]; comment: string };
  whisper: {
    nav: { home: string; insights: string; upload: string; dictionary: string; settings: string };
    search: string;
    today: string;
    showDiscarded: string;
    clearAll: string;
    rawTranscript: string;
    copied: string;
    query: string;
    history: { time: string; text: string }[];
    upload: {
      title: string;
      drop: string;
      formats: string;
      files: string[];
      progress: (done: number, total: number) => string;
      complete: string;
      saved: string;
    };
    insights: {
      title: string;
      words: string;
      streak: string;
      wpm: string;
      dictations: string;
      allTime: string;
      days: (count: number) => string;
      activity: string;
      onDevice: string;
      values: { words: number; streak: number; wpm: number; dictations: number };
    };
    settings: {
      speechToText: string;
      shared: string;
      endpoint: string;
      model: string;
      cleanup: string;
      enableCleanup: string;
      asrUrl: string;
      asrModel: string;
      cleanupUrl: string;
      cleanupModel: string;
    };
  };
  outro: { install: string; link: string };
}

const en: Copy = {
  lang: "en",
  font: '-apple-system, "SF Pro Display", "Helvetica Neue", sans-serif',
  gap: " ",
  slogan: "Speak. It's typed.",
  subtitle: "Whisper · dictation for your Mac, on your own servers",
  key: { doubleTap: "Double-tap", hold: "Hold" },
  clock: {
    morning: "7:45 AM",
    chat: "9:30 AM",
    snippet: "12:15 PM",
    hold: "3:05 PM",
    upload: "6:40 PM",
    night: "10:20 PM",
  },
  menuClock: {
    morning: "Mon 7:45 AM",
    chat: "Mon 9:30 AM",
    snippet: "Mon 12:15 PM",
    hold: "Mon 3:05 PM",
    upload: "Mon 6:40 PM",
    night: "Mon 10:20 PM",
  },
  caption: {
    dictate: "Double-tap fn, speak, and it's pasted",
    cleanup: "Fillers gone, punctuation in",
    learn: "Fix a word once. Whisper learns it.",
    snippet: "Say a trigger, get the whole snippet",
    hold: "Hold fn to talk, let go to paste",
    upload: "Drop in recordings, transcribe in batches",
    history: "Every dictation, searchable",
    insights: "Your day, in words",
    servers: "Your servers. Your data.",
    serversNote: "Connect any OpenAI-compatible speech and cleanup server",
  },
  saidLabel: "You said",
  cleanedLabel: "Pasted",
  mail: {
    app: "Mail",
    to: "To:",
    toName: "Maya Chen",
    subject: "Subject:",
    subjectText: "Re: Friday launch",
    greeting: "Hi Maya,",
    spoken: [
      { text: "um", filler: true },
      { text: "so", filler: true },
      { text: "Friday" },
      { text: "works" },
      { text: "for" },
      { text: "me" },
      { text: "uh", filler: true },
      { text: "let's" },
      { text: "ship" },
      { text: "at" },
      { text: "ten" },
      { text: "and" },
      { text: "I'll" },
      { text: "send" },
      { text: "the" },
      { text: "notes" },
      { text: "after" },
    ],
    cleaned: "Friday works for me. Let's ship at 10, and I'll send the notes after.",
  },
  chat: {
    app: "Team chat",
    channel: "design-review",
    channels: ["general", "design-review", "launch", "random"],
    teammate: "Leo",
    earlier: [
      { name: "Mia", time: "9:12", text: "New onboarding mockups are up in the design folder." },
      { name: "Leo", time: "9:20", text: "Love the empty states. Let's review them together." },
    ],
    question: "Who's joining the design review at 11?",
    you: "You",
    heard: "I'll join. Can you add Shivon too?",
    wrong: "Shivon",
    right: "Siobhan",
    toast: "Added “Siobhan” to your dictionary",
    placeholder: "Message #design-review",
  },
  snippet: {
    app: "Messages",
    contact: "Priya",
    earlier: [
      { mine: false, text: "That ramen place near the office reopened!" },
      { mine: true, text: "No way, we have to go 🍜" },
    ],
    incoming: "Lunch next week? When are you free?",
    said: "Sounds great, grab any slot here: cal link",
    trigger: "cal link",
    expansion: "cal.com/alex/30min",
    before: "Sounds great, grab any slot here: ",
  },
  hold: {
    file: "upload.ts",
    code: [
      "export async function upload(file: File) {",
      "  const url = await signedUrl(file.name);",
      "",
      "  return retry(() => put(url, file), {",
      "    attempts: 5,",
      '    backoff: "exponential",',
      "  });",
      "}",
    ],
    comment: "  // Retry failed uploads with exponential backoff",
  },
  whisper: {
    nav: {
      home: "Home",
      insights: "Insights",
      upload: "Upload",
      dictionary: "Dictionary",
      settings: "Settings",
    },
    search: "Transcripts",
    today: "Today",
    showDiscarded: "Show failed and discarded",
    clearAll: "Clear all",
    rawTranscript: "Raw Transcript",
    copied: "Copied!",
    query: "launch",
    history: [
      { time: "18:41", text: "Team sync: we agreed to move the launch review to Thursday." },
      { time: "15:05", text: "// Retry failed uploads with exponential backoff" },
      { time: "12:15", text: "Sounds great, grab any slot here: cal.com/alex/30min" },
      { time: "09:30", text: "I'll join. Can you add Siobhan too?" },
      {
        time: "07:45",
        text: "Friday works for me. Let's ship at 10, and I'll send the notes after.",
      },
    ],
    upload: {
      title: "Upload Audio",
      drop: "Drop an audio or video file, or click to browse",
      formats: "MP3, WAV, M4A, MP4, MOV and most other audio or video formats",
      files: ["team-sync.m4a", "customer-interview.mp4", "voice-memo.m4a"],
      progress: (done, total) => `${done}/${total} completed`,
      complete: "Transcription complete",
      saved: "Saved to history",
    },
    insights: {
      title: "Your Usage",
      words: "Words spoken",
      streak: "Current streak",
      wpm: "Words per minute",
      dictations: "Dictations",
      allTime: "All time",
      days: (count) => `${count} days`,
      activity: "Speaking activity",
      onDevice: "On this device",
      values: { words: 18420, streak: 12, wpm: 148, dictations: 37 },
    },
    settings: {
      speechToText: "Speech-to-Text",
      shared: "Dictation and file transcription share these settings.",
      endpoint: "Endpoint URL",
      model: "Model",
      cleanup: "Text cleanup",
      enableCleanup: "Enable text cleanup",
      asrUrl: "http://localhost:8000/v1",
      asrModel: "whisper-large-v3-turbo",
      cleanupUrl: "http://192.168.1.20:11434/v1",
      cleanupModel: "qwen3:8b",
    },
  },
  outro: {
    install: "brew install --cask softmaxe/tap/whisper",
    link: "github.com/softmaxe/whisper",
  },
};

const zh: Copy = {
  lang: "zh-CN",
  font: '-apple-system, "PingFang SC", "Hiragino Sans GB", sans-serif',
  gap: "",
  slogan: "开口，即成文。",
  subtitle: "Whisper · Mac 听写，跑在你自己的服务器上",
  key: { doubleTap: "双击", hold: "按住" },
  clock: {
    morning: "上午 7:45",
    chat: "上午 9:30",
    snippet: "中午 12:15",
    hold: "下午 3:05",
    upload: "傍晚 6:40",
    night: "晚上 10:20",
  },
  menuClock: {
    morning: "周一 上午7:45",
    chat: "周一 上午9:30",
    snippet: "周一 中午12:15",
    hold: "周一 下午3:05",
    upload: "周一 下午6:40",
    night: "周一 晚上10:20",
  },
  caption: {
    dictate: "双击 fn，开口说，自动粘贴",
    cleanup: "去掉语气词，补全标点",
    learn: "改一次，Whisper 就记住了",
    snippet: "说出触发词，展开整段内容",
    hold: "按住 fn 说话，松开即粘贴",
    upload: "拖入录音，批量转写",
    history: "每一次听写，都能搜到",
    insights: "今天的你，说了这么多",
    servers: "你的服务器，你的数据。",
    serversNote: "接入任意 OpenAI 兼容的语音识别与文本整理服务",
  },
  saidLabel: "你说的",
  cleanedLabel: "粘贴的",
  mail: {
    app: "邮件",
    to: "收件人：",
    toName: "陈雅",
    subject: "主题：",
    subjectText: "回复：周五发布",
    greeting: "陈雅你好，",
    spoken: [
      { text: "嗯", filler: true },
      { text: "那个", filler: true },
      { text: "周五" },
      { text: "可以的" },
      { text: "呃", filler: true },
      { text: "我们" },
      { text: "十点" },
      { text: "发布" },
      { text: "然后" },
      { text: "我会" },
      { text: "把" },
      { text: "会议纪要" },
      { text: "发给大家" },
    ],
    cleaned: "周五可以。我们十点发布，之后我会把会议纪要发给大家。",
  },
  chat: {
    app: "团队聊天",
    channel: "设计评审",
    channels: ["综合", "设计评审", "发布", "闲聊"],
    teammate: "李奥",
    earlier: [
      { name: "米娅", time: "9:12", text: "新版引导页的设计稿已经放到设计文件夹了。" },
      { name: "李奥", time: "9:20", text: "空状态做得很好，我们一起过一遍吧。" },
    ],
    question: "11 点的设计评审谁来参加？",
    you: "我",
    heard: "我参加。记得带上苏帕贝斯的迁移方案。",
    wrong: "苏帕贝斯",
    right: "Supabase",
    toast: "已将 “Supabase” 添加到你的词典",
    placeholder: "发送消息到 #设计评审",
  },
  snippet: {
    app: "信息",
    contact: "小雨",
    earlier: [
      { mine: false, text: "公司楼下那家拉面店重新开业啦！" },
      { mine: true, text: "真的吗，必须去 🍜" },
    ],
    incoming: "下周一起吃个午饭？你哪天有空？",
    said: "好呀，在这里挑个时间：我的日程链接",
    trigger: "我的日程链接",
    expansion: "cal.com/alex/30min",
    before: "好呀，在这里挑个时间：",
  },
  hold: {
    file: "upload.ts",
    code: [
      "export async function upload(file: File) {",
      "  const url = await signedUrl(file.name);",
      "",
      "  return retry(() => put(url, file), {",
      "    attempts: 5,",
      '    backoff: "exponential",',
      "  });",
      "}",
    ],
    comment: "  // 上传失败时按指数退避重试",
  },
  whisper: {
    nav: { home: "首页", insights: "统计", upload: "上传", dictionary: "词典", settings: "设置" },
    search: "转录",
    today: "今天",
    showDiscarded: "显示失败和已放弃的记录",
    clearAll: "全部清除",
    rawTranscript: "原始转录",
    copied: "已复制！",
    query: "发布",
    history: [
      { time: "18:41", text: "周会结论：发布评审改到周四。" },
      { time: "15:05", text: "// 上传失败时按指数退避重试" },
      { time: "12:15", text: "好呀，在这里挑个时间：cal.com/alex/30min" },
      { time: "09:30", text: "我参加。记得带上Supabase的迁移方案。" },
      { time: "07:45", text: "周五可以。我们十点发布，之后我会把会议纪要发给大家。" },
    ],
    upload: {
      title: "上传音频",
      drop: "拖放音频或视频文件，或点击浏览",
      formats: "MP3、WAV、M4A、MP4、MOV 及大多数其他音频或视频格式",
      files: ["周会录音.m4a", "用户访谈.mp4", "语音备忘.m4a"],
      progress: (done, total) => `${done}/${total} 已完成`,
      complete: "转录完成",
      saved: "已保存到历史记录",
    },
    insights: {
      title: "你的使用情况",
      words: "已听写字数",
      streak: "当前连续天数",
      wpm: "每分钟字数",
      dictations: "听写次数",
      allTime: "全部时间",
      days: (count) => `${count} 天`,
      activity: "听写活跃度",
      onDevice: "仅在本设备",
      values: { words: 32680, streak: 12, wpm: 196, dictations: 37 },
    },
    settings: {
      speechToText: "语音转文字",
      shared: "听写与文件转录共用此配置。",
      endpoint: "端点 URL",
      model: "模型",
      cleanup: "文本整理",
      enableCleanup: "启用文本整理",
      asrUrl: "http://localhost:8000/v1",
      asrModel: "whisper-large-v3-turbo",
      cleanupUrl: "http://192.168.1.20:11434/v1",
      cleanupModel: "qwen3:8b",
    },
  },
  outro: {
    install: "brew install --cask softmaxe/tap/whisper",
    link: "github.com/softmaxe/whisper",
  },
};

export const COPY = { en, "zh-CN": zh } as const;
export type Lang = keyof typeof COPY;

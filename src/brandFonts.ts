// Registers the bundled JetBrains Mono faces without network access.
const FACES: Record<string, { family: string; weight: string }> = {
  "JetBrainsMono-Regular": { family: "JetBrains Mono", weight: "400" },
  "JetBrainsMono-Medium": { family: "JetBrains Mono", weight: "500" },
  "JetBrainsMono-SemiBold": { family: "JetBrains Mono", weight: "600" },
  "JetBrainsMono-Bold": { family: "JetBrains Mono", weight: "700" },
};

const fontUrls = import.meta.glob("./assets/fonts/jetbrains-mono/*.woff2", {
  eager: true,
  query: "?url",
  import: "default",
}) as Record<string, string>;

const rules = Object.entries(fontUrls).flatMap(([file, url]) => {
  const face = FACES[file.slice(file.lastIndexOf("/") + 1, -".woff2".length)];
  return face
    ? [
        `@font-face{font-family:"${face.family}";font-style:normal;font-weight:${face.weight};font-display:swap;src:url("${url}") format("woff2")}`,
      ]
    : [];
});

if (rules.length) {
  const style = document.createElement("style");
  style.textContent = rules.join("\n");
  document.head.append(style);
}

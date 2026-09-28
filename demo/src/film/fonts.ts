import { cancelRender, continueRender, delayRender, staticFile } from "remotion";

/** Bundled LXGW WenKai subset (OFL, see public/fonts/OFL.txt), used for Captions in both cuts. */
export const HAND_FONT = "LXGW WenKai";
const HAND_FONT_FILE = "fonts/LXGWWenKai-Regular.subset.woff2";

let started = false;

/** Registers the bundled fonts and blocks rendering until they are ready. */
export function loadFonts(): void {
  if (started || typeof document === "undefined") return;
  started = true;
  const handle = delayRender(`Loading ${HAND_FONT}`);
  const face = new FontFace(HAND_FONT, `url(${staticFile(HAND_FONT_FILE)}) format("woff2")`);
  face
    .load()
    .then((loaded) => {
      document.fonts.add(loaded);
      continueRender(handle);
    })
    .catch((err) => cancelRender(err));
}

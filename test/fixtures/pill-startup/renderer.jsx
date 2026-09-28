import React from "react";
import { createRoot } from "react-dom/client";
import { flushSync } from "react-dom";
import { VoicePill } from "../../../src/components/dictation/VoicePill";
import "./styles.css";

const root = createRoot(document.getElementById("root"));
Object.assign(document.getElementById("root").style, {
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  width: "200px",
  height: "80px",
});
let level = null;
let reads = 0;
const getAudioLevel = () => {
  reads += 1;
  return level;
};
const bars = () => [...document.querySelectorAll(".voice-flow-bar")];
window.pillTest = {
  render(state, input = null) {
    level = input;
    flushSync(() =>
      root.render(<VoicePill variant="floating" state={state} getAudioLevel={getAudioLevel} />)
    );
  },
  block(ms) {
    const start = performance.now();
    while (performance.now() - start < ms) {
      // Simulate synchronous native audio initialization on the renderer thread.
    }
    return performance.now() - start;
  },
  sample() {
    return {
      reads,
      height: bars().reduce((sum, bar) => sum + bar.getBoundingClientRect().height, 0),
      animations: bars().flatMap((bar) => bar.getAnimations()).length,
    };
  },
  unmount() {
    const animations = bars().flatMap((bar) => bar.getAnimations());
    flushSync(() => root.unmount());
    return animations.every((animation) => animation.playState === "idle");
  },
};

import { AbsoluteFill, Sequence } from "remotion";
import { allCaptions, toFrame } from "../../../timeline/index.ts";
import { useLang } from "../lang.tsx";
import { HandwrittenCaption } from "./HandwrittenCaption.tsx";

/** Every Caption in the timeline, in the cut's language, each in a Sequence spanning its own start..end. */
export const Captions: React.FC = () => {
  const lang = useLang();
  return (
    <AbsoluteFill>
      {allCaptions().map((c) => (
        <Sequence
          key={c.id}
          name={`Caption ${c.id}`}
          from={toFrame(c.start)}
          durationInFrames={toFrame(c.end) - toFrame(c.start)}
          layout="none"
        >
          <HandwrittenCaption caption={c} text={c.text[lang]} />
        </Sequence>
      ))}
    </AbsoluteFill>
  );
};

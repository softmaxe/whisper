import { Config } from "@remotion/cli/config";

// Settings for Remotion Studio and ad-hoc `remotion still` renders.
// `npm run build` passes its own options to the renderer.
Config.setVideoImageFormat("jpeg");
Config.setJpegQuality(92);
Config.setOverwriteOutput(true);

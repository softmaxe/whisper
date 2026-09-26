import { Config } from "@remotion/cli/config";

Config.setVideoImageFormat("jpeg");
Config.setJpegQuality(92);
Config.setOverwriteOutput(true);
Config.setCodec("h264");
// CRF 21 and 192 kbps audio keep each video under GitHub's 10 MB upload
// limit for README embeds.
Config.setCrf(21);
Config.setAudioBitrate("192k");
Config.setPixelFormat("yuv420p");

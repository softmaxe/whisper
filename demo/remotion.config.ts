import { Config } from "@remotion/cli/config";

Config.setVideoImageFormat("jpeg");
Config.setJpegQuality(92);
Config.setOverwriteOutput(true);
Config.setCodec("h264");
Config.setCrf(21);
// Keeps each video under GitHub's 10 MB upload limit for README embeds.
Config.setAudioBitrate("192k");
Config.setPixelFormat("yuv420p");

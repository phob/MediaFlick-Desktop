// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type UpdateAsset = {
  name: string,
  browserDownloadUrl: string,
  size: number | null,
};

export type UpdateRelease = {
  version: string,
  automaticInstall: boolean,
  asset: UpdateAsset | null,
};

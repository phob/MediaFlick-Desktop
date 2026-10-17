// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type PersonIdentity = {
  jellyfinId: string,
  tmdbId: number | null,
  name: string,
  imageTag: string | null,
};

export type PersonResolution = {
  person: PersonIdentity | null,
  candidates: Array<PersonIdentity>,
  ambiguous: boolean,
};

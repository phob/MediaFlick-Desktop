// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type ItemAbout = {
  overview: string | null,
  criticRating: number | null,
  people: Array<Person>,
  tags: Array<string>,
  studios: Array<string>,
};

export type ItemSynopsis = {
  overview: string | null,
};

export type Person = {
  id: string | null,
  name: string | null,
  role: string | null,
  type: string | null,
  imageTag: string | null,
};

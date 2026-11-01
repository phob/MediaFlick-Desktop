// Generated from src/shell/cef/api/wire.rs by `just wire-types`. Do not edit.

export type SeerrCompany = {
  id: number,
  name: string,
};

export type SeerrCompanyPage = {
  page: number,
  totalPages: number,
  totalResults: number,
  results: Array<SeerrCompany>,
};

export type SeerrNetworkList = {
  networks: Array<SeerrCompany>,
};

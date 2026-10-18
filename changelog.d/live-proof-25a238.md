### Changed

- Writes to the account-scoped library cache now require a proof that the signed-in account is still the one the data was fetched for, so a new code path cannot store one account's answers after a switch or sign-out. Behaviour is unchanged.

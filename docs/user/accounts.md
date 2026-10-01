# MCPanel accounts

An MCPanel account is optional. Servers and other local features work without signing in. Accounts currently provide identity only; they do not sync settings, servers, or backups between computers.

## Create an account or sign in

Use the Account step during first-run setup, or open **Settings → Account** later. Depending on the build, you can create an account with an email and password or continue with Google. MCPanel sends email and password credentials to Firebase over HTTPS; it does not store your password. A successful email sign-up sends a verification message. Follow its link, then use **Refresh** in Settings to update the verification status. If the message does not arrive, use **Resend verification**.

Use **Forgot password?** on the sign-in form to request a reset link. For Google sign-in, MCPanel opens your browser and returns to the app after approval. You can sign out from **Settings → Account**.

If the account service is not configured in the installed build, MCPanel shows that accounts are unavailable. You can continue using the app without an account.

## What MCPanel stores

The refresh token is kept in the Windows Credential Manager. MCPanel caches your account name, email, verification status, provider, and profile photo URL in local settings so it can show your account while offline. Signing out removes the stored token and cached profile. Account sign-in does not upload server files or back up data to the cloud.

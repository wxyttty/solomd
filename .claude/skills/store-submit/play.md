# Google Play Console

App id `4973693003540583322`, package `app.solomd`. Console lives under
`https://play.google.com/console/u/0/developers/8662341371772988450/app/4973693003540583322/`.

Since 4.12.0 the app is on the **production track** and needs no further
permission gate — the 12-testers-for-14-days requirement is behind us.

```bash
S=.claude/skills/store-submit
export UNZOO_TAB=$($S/unzoo.sh find-tab play.google.com/console)
```

## The upload, which is the part that fights back

The file input is hidden: `input[type=file][accept=".aab"]`. Three things all
have to happen, in order, or nothing does.

```bash
$S/unzoo.sh upload 'input[type=file][accept=".aab"]' dist-android/SoloMD_X.Y.Z.aab
```

- **A synthetic `.click()` on the input does not open anything**, and clicking
  the label does not either. If you need the native picker, click the visible
  upload button by coordinate (`box` the `mdc-button`, then `click`). Usually
  you do not need it: `set_input_files` with `trusted:true` skips the picker.
- **Setting the files does not start the upload.** The Angular uploader listens
  for `change`. `set_input_files` with `trusted:true` fires a real one itself —
  `unzoo.sh upload` only dispatches a synthetic `change` if no native one
  arrived. Do not add your own dispatch on top (see the next bullet).
- **★ "0 B of NN MB" that never moves is a duplicate `change`, not a dead
  upload.** `unzoo.sh upload` used to dispatch `change` unconditionally, so the
  page got two — one trusted, one synthetic — for the same file. Play then
  renders the progress model built for the second while the first is the one
  actually streaming bytes, and it sits at "0 B" from start to finish. It also
  hides the *real* error when the upload finishes and the server rejects it.
  This burned two sessions on 4.13.0. Fixed in `unzoo.sh`; if you ever see
  "0 B" again, count the change events before blaming the network:
  `document.addEventListener('change', e => ..., true)`.
- **Trust the XHR, not the progress text.** Hook
  `XMLHttpRequest.prototype.send` and listen on `xhr.upload` to see real bytes.
  `performance.getEntriesByType("resource")` shows *nothing* for this upload —
  it is issued from `uploader.client.dartjs.v3/uploader.js`, outside the main
  context — so an empty resource list is not evidence that no request was made.
  `POST /api/v1/network/requests` (unzoo's CDP capture) does show it.
- **Leaving the page cancels the upload.** Do not navigate while it runs.

## ★ Check the AAB is signed before you upload it

Play rejects an unsigned bundle with 「所有上传的软件包都必须签名」 — and it does
so *after* the whole 39 MB is uploaded, attached to the file row as a small
`error` icon that is easy to read as a network failure. Gradle produces an
unsigned release artifact **silently** when the `ANDROID_KEYSTORE_*` env vars
are missing, which is what happens if you run `./gradlew bundleUniversalRelease`
directly instead of `scripts/build-android.sh`. Both ends now refuse
(the gradle config throws, the script asserts), but verify anyway:

```bash
python3 -c "import zipfile,sys; z=zipfile.ZipFile(sys.argv[1]); \
print([n for n in z.namelist() if n.startswith('META-INF/')])" dist/SoloMD_X.Y.Z_play.aab
# want: MANIFEST.MF + SOLOMD.SF + SOLOMD.RSA
```

To sign one that is already built (keeps the versionCode):

```bash
set -a; source .env.local; set +a
jarsigner -keystore "$ANDROID_KEYSTORE_PATH" -storepass "$ANDROID_KEYSTORE_PASS" \
  -keypass "$ANDROID_KEY_PASS" -sigalg SHA256withRSA -digestalg SHA-256 \
  the.aab "$ANDROID_KEY_ALIAS"
jarsigner -verify the.aab
```

Confirm the cert matches one Play has already accepted — a wrong key is
rejected the same way as no key:

```bash
unzip -p the.aab 'META-INF/*.RSA' | openssl pkcs7 -inform DER -print_certs \
  | openssl x509 -noout -fingerprint -sha256
```

The AAB is at `app/src-tauri/gen/android/app/build/outputs/bundle/universalRelease/`
after a release build.

Watch progress by polling text rather than screenshotting:

```bash
while :; do $S/unzoo.sh dom-text | grep -oE '正在上传|已上传|错误' | head -3; sleep 10; done
# dom-text, not text — see SKILL.md. `text` empties out whenever the window is
# not on screen, which is exactly the situation a polling loop runs in.
```

## Version codes

`versionCode` is `X.YY.ZZZ` flattened — 4.12.0 is `4012000`. A code already used
by any earlier release is rejected, and the rejection banner sticks around in
the draft until you clear the bad attachment.

## Creating the release

1. Track → **创建新的发布版本**.
2. Attach the AAB (above). Wait for Google's optimisation to report target SDK
   and ABIs.
3. Release notes: `zh-CN` at minimum, in the `<zh-CN>…</zh-CN>` block.
4. **Countries.** A fresh production track has *none*, and the error reads
   「您还没有为此轨道选择任何国家或地区」. Go to
   `tracks/<id>?tab=countryAvailability` → 添加国家/地区 → tick **选择所有行**
   (that is the literal aria-label; it selects all 177) → **保存**. The
   「修改国家/地区」button is greyed out — do not wait on it. The track summary
   must then read 「177 个国家/地区」.
5. **16 KB page size.** 「您的应用不支持 16 KB 内存页面大小」offers 「仍然继续」,
   after which it is marked 「错误(在此发布版本中被忽略)」and ships. It is a real
   deadline for target SDK 35+, not a false alarm — fix it before it becomes
   blocking. (A missing native-symbols upload is a genuine optional warning.)
6. Submit for review. Review has completed in well under an hour despite the
   quoted seven days.

## ★ Managed publishing is ON

Approval does **not** put the release live. Go back to the release overview and
press publish. This is a checkpoint, not a fault — and the reason a release can
sit "approved" for days while everyone believes it shipped.

## ★ The publish queue is shared

「已可发布的更改」can contain changes that are not yours — historically a stale
open-testing release. Two things to know:

- 「移除更改」removes **everything**, including your approved release, sending it
  back through review.
- A single item can only be dropped from its own track's 「舍弃版本」button.

Read the queue before publishing, and if something unrelated is in it, ask
rather than guessing.

## ★ An empty modal means the Google session is half-dead

A dialog that opens blank is not a page bug — the session is partially expired.
Fix: sign in again and open a **new tab**; the old tab stays broken. Do not try
to repair it with cookie surgery.

## Signing, and why Play and GitHub builds cannot update each other

Play re-signs with its own app-signing key, which is not our upload key, so a
sideloaded APK and a Play install are different applications to Android and
cannot cross-update. The in-app update check reads GitHub's latest release and
is blind to Play-only versions. See `reference_play_signing_and_tracks`.

## Verify, then report

```bash
$S/unzoo.sh nav 'https://play.google.com/console/u/0/developers/8662341371772988450/app/4973693003540583322/tracks/production'
$S/unzoo.sh dom-text | head -40
```

The release is live when the track shows the version code as 已发布/全面发布
with a country count — not when the submit button went green.

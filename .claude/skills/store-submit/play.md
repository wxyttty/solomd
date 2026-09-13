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
  for `change`; `unzoo.sh upload` dispatches it for you.
- **Leaving the page cancels the upload.** Do not navigate while it runs.

The AAB is at `app/src-tauri/gen/android/app/build/outputs/bundle/universalRelease/`
after a release build.

Watch progress by polling text rather than screenshotting:

```bash
while :; do $S/unzoo.sh text | grep -iE '正在上传|上传|%|已上传|错误' | head -3; sleep 10; done
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
$S/unzoo.sh text | head -40
```

The release is live when the track shows the version code as 已发布/全面发布
with a country count — not when the submit button went green.

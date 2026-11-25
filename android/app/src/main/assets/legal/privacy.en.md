# Peerage privacy policy

Version 1 · effective 25 November 2026.

## In short

Peerage has no servers. Messages, contacts and usage information never reach the developers: there is nowhere for them to go. There is no sign-up, no phone number and no email. The app has no analytics, no ads and no automatic crash reports.

## What is stored on the device

- Identity: the key and the 24 words that restore it. No copy exists anywhere else — without the words the identity cannot be recovered.
- Messages, attachments, contacts, groups and settings. The database and group files are encrypted; the key is kept in the Android Keystore, and if a password is set, it is needed too.
- The app log — without names, message text or keys. It leaves the device only if saved with “Save log” and sent by hand.

Uninstalling the app deletes this data from the device.

## What others see

Messages, files and calls go directly between devices over the Yggdrasil network and are end-to-end encrypted: only the recipient can read them.

- Contacts see the name and avatar, the network address and the “reachable” status, which delivery needs. Read receipts and the typing indicator are visible only if turned on in privacy settings (off by default). The “online” status — while the app is open — is on by default and is turned off in the same place.
- Group members see the group's messages, its member list and the members' keys.
- Yggdrasil entry nodes — public nodes from the app's list, or custom ones — see the device's IP address, connection times and traffic volume, but not the content. Public nodes are run by third parties. The list can be turned off and custom nodes added.
- Network nodes along a packet's path see the sender's and recipient's network addresses and the packet size. Sizes are padded, so text, files and calls cannot be told apart by size.
- In “Network node” mode the phone forwards other people's encrypted mesh traffic; its content stays inaccessible.

## Updates

The update check is off until consent is given (the switch in About). When on, it asks GitHub for the list of releases once a day; GitHub sees the IP address and the time of the request. An update is downloaded from GitHub on tap.

## Permissions

- Microphone and camera — calls, voice messages, taking photos and scanning QR codes.
- Photos and videos — only the ones picked for sending.
- Notifications, background activity and start after reboot — receiving messages without a push server.
- Installing apps — installing a downloaded update.
- Biometrics — opening chats with a fingerprint, if turned on.

## Changes

A new version of this policy ships with an app release and is noted in “What's new”. The history of changes is in the public repository.

## Contact

Questions and remarks — via issues in the repository: github.com/peerage-chat/peerage

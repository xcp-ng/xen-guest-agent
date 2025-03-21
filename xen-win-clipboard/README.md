# xen-win-clipboard per-session clipboard agent

xen-win-clipboard is a per-session agent that communicates with xen-guest-agent's clipboard provider to exchange clipboard data.

It uses a clipboard format listener to watch its clipboard contents, then passes them over to the clipboard provider over named pipes.
The clipboard provider accepts connections from all sessions, but only allows the console session to publish/subscribe to clipboard contents.

## Pipe talker message format

UTF-8 string containing clipboard contents.

Note that due to xenstore/vnc limitations, we currently strip away characters not in the 0x20..0xff range.

This is eventually converted into UTF-16 by xen-win-clipboard for use with CF_UNICODETEXT.

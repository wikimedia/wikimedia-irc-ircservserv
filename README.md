ircservserv
===========

Because we need services for services.

Manages IRC channel configuration from declarative toml files. In the future
this will cover:

* /cs flags
* /mode +b $j: (global bans)
* /mode +I $a: (invexes)
* channel modes (private vs public)
* ???

Example, initial setup of #wikimedia-kawaii:
```
01:08:50 <@legoktm> !issync #wikimedia-kawaii
01:08:50 <ircservserv> Syncing #wikimedia-kawaii
01:08:51 <ircservserv> Set /cs flags #wikimedia-kawaii quiddity +AFRefiorstv
01:08:51 <ircservserv> Set /cs flags #wikimedia-kawaii p858snake +Afiortv
01:08:52 <ircservserv> Set /cs flags #wikimedia-kawaii wmopbot +o
01:08:53 <ircservserv> Set /cs flags #wikimedia-kawaii *!*@libera/staff/* +o
```

Does it think it applied everything?
```
01:09:52 <@legoktm> !issync #wikimedia-kawaii
01:09:52 <ircservserv> Syncing #wikimedia-kawaii
01:09:52 <ircservserv> No flag updates for #wikimedia-kawaii
```

Now I added someone as a op by editing the toml config:
```
01:10:54 <@legoktm> !issync #wikimedia-kawaii
01:10:54 <ircservserv> Syncing #wikimedia-kawaii
01:10:55 <ircservserv> Set /cs flags #wikimedia-kawaii ashley +Aiotv
```


(C) 2021 Kunal Mehta under the GPL v3, or any later version.

the main idea ... 


so in the first place we are looking at organising to a point so that it works by getting the info from the config files 

---

-> the first thing is that we need 2 config files 
   ->  one for the server and one for the client 

now let's talk about the syntax for the config files

the main sytax is that the config files are written in TOML 


this thing is for the server config
```toml
[server]
port = 8081

[dir]
path = "./"

[chunks]
size = 4096
// something like this will be used to denote this thing

```

this thing is for the client config 

```toml
[client]
username = ""
repo = ""

[ignore]
files = ["something.txt", "dir/"]


// and maybe more 
```


deltas and shit 

the main idea here which i really need is that there should be something like stages and then deltas ... this will exist in both the local and remote



now lets go for the commands ... 

gyt -> prefix as pr 


pr init . -> initialises the repository
pr status -> shows the status of the repository

pr add  -> adds files to the repository ... so what add does is that it adds the files to the staging area like a bucket  

pr push -> pushes the changes to the remote
pr push local -> pushes the changes to the local so once it is all pushed to the local and then u can push it to the remote as a bundle with a commit message for that bundle... the bundle is a bucket for a set of changes

pr pull -> pulls the changes from the remote
pr pull <commit> -> pulls the changes from the remote (specific commit)


pr snip bot <commit> -> this will delete the commits and the pre stages from the commit specified by the user
pr snip top -> this will delete the stages from the current thingy to the top

this we can have with multiple branches

we will handle that later so we are chill now

this will be good ... just for now .. and we can take it from here

---------------------------------------------------------------------------------------------------------------------------
so now all i need is the thing which should i start coding with ... i need a flow thingy in here ... i need to make this as good as possible before it goes out to the raspberry pi
---------------------------------------------------------------------------------------------------------------------------

---
some
---

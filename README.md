<p float="left">
   <img valign="middle" alt="Stage logo" src="./icons/64x64/com.github.aganzha.stage.png" width="32">
   <strong>Stage</strong> -
   <span>Git GUI client for linux desktops inspired by Magit</span>
</p>

## Installation
```sh
flatpak install -u https://github.com/aganzha/stage/raw/master/stage.flatpakref
```

## Usage
Start within your DE via dash, activities, software search etc by typing Stage, or by flatpak:

```sh
flatpak run com.github.aganzha.stage
```

### Staging

Open git repository. Stage will watch for changes and display current repository status in form of Diff.

- Expand/collapse underlying files and hunks by pressing TAB or SPACE or clicking expandanle items on screen.

- Stage selected files or hunks or all changes by pressing ENTER or S or double clicking items on screen

- Unstage staged changes by pressing U or double clicking while cursor is on staged items.

- Kill changes by pressing K while cursor is on unstaged items.



### Commit/Push/Pull
### Branches
#### Merging
##### Resolving conflicts
#### Rebasing
### Stash
### Log
### Cherry-picking
### Reverting
export let nixRt = {
    export: (anchor, path) => {
        console.log('called RT.export with anchor=' + anchor + ' path=' + path);
        return anchor + '://' + path;
    },
    import: (path) => {
        console.log('called RT.import with path=' + path);
        throw Error("no imports supported");
    }
}

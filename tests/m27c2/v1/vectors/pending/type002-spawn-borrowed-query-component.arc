pub component Item {
    pub value: i32,
}

pub system InvalidSpawn(
    items: query [mut Item],
    cmd: commands,
) requires {} throws {} {
    for item in items {
        cmd.spawn { item };
    }
}

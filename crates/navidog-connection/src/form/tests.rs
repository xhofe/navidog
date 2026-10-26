use super::validation::{member_list, number_value, port_or_default, port_value};

#[test]
fn invalid_ports_and_server_lines_are_errors() {
    assert_eq!(port_value("", "Port").unwrap(), None);
    assert_eq!(port_value("3306", "Port").unwrap(), Some(3306));
    assert!(port_value("3306x", "Port").is_err());
    assert!(port_value("70000", "Port").is_err());
    assert_eq!(port_or_default("", 22, "Port").unwrap(), 22);
    assert_eq!(number_value("30", "Timeout").unwrap(), Some(30));
    assert!(number_value("-1", "Timeout").is_err());
    assert_eq!(member_list("db1:3306\n[::1]:3307\n").unwrap().len(), 2);
    assert!(member_list("db1:3306\nnot a host:port\n").is_err());
}

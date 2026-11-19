//! The statements a test runs to act as a role without a superuser's
//! privileges.

use crate::{
    command::{Command, rendering::RenderedCommand},
    identifier::quoted_iden,
};
use sea_query::{DynIden, IntoIden};

/// `CREATE ROLE <role> NOLOGIN`, a role a superuser's session takes with
/// `SET ROLE`.
#[derive(Clone, Debug)]
pub struct RoleCreation(DynIden);

impl RoleCreation {
    /// Create `role`.
    pub fn new(role: impl IntoIden) -> RoleCreation {
        RoleCreation(role.into_iden())
    }
}

impl RenderedCommand for RoleCreation {
    fn rendered(&self) -> String {
        format!("CREATE ROLE {} NOLOGIN", quoted_iden(&self.0))
    }
}

impl Command for RoleCreation {}

/// `DROP OWNED BY <role>; DROP ROLE <role>`, run in the one database the role
/// holds privileges in.
#[derive(Clone, Debug)]
pub struct RoleRemoval(DynIden);

impl RoleRemoval {
    /// Revoke what `role` holds in the current database, then drop it.
    pub fn new(role: impl IntoIden) -> RoleRemoval {
        RoleRemoval(role.into_iden())
    }
}

impl RenderedCommand for RoleRemoval {
    fn rendered(&self) -> String {
        format!("DROP OWNED BY {role}; DROP ROLE {role}", role = quoted_iden(&self.0))
    }
}

impl Command for RoleRemoval {}

/// `GRANT SELECT ON ALL TABLES IN SCHEMA public TO <role>`, so the role reads
/// every table the current database's `public` schema holds.
#[derive(Clone, Debug)]
pub struct PublicTablesReadGrant(DynIden);

impl PublicTablesReadGrant {
    /// Let `role` read the tables of `public`.
    pub fn new(role: impl IntoIden) -> PublicTablesReadGrant {
        PublicTablesReadGrant(role.into_iden())
    }
}

impl RenderedCommand for PublicTablesReadGrant {
    fn rendered(&self) -> String {
        format!("GRANT SELECT ON ALL TABLES IN SCHEMA \"public\" TO {}", quoted_iden(&self.0))
    }
}

impl Command for PublicTablesReadGrant {}

#[cfg(test)]
mod tests {
    use crate::{
        command::rendering::RenderedCommand,
        test_role::{PublicTablesReadGrant, RoleCreation, RoleRemoval},
    };
    use pretty_assertions::assert_eq;
    use sea_query::Alias;

    #[test]
    fn each_statement_names_its_role_as_a_quoted_identifier() {
        assert_eq!(RoleCreation::new(Alias::new("reader")).rendered(), r#"CREATE ROLE "reader" NOLOGIN"#);
        assert_eq!(
            RoleRemoval::new(Alias::new("reader")).rendered(),
            r#"DROP OWNED BY "reader"; DROP ROLE "reader""#
        );
        assert_eq!(
            PublicTablesReadGrant::new(Alias::new("reader")).rendered(),
            r#"GRANT SELECT ON ALL TABLES IN SCHEMA "public" TO "reader""#
        );
    }
}

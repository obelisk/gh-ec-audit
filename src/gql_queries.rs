/// GQL query to recover a user's email address from their GitHub login
pub const USER2EMAIL: &str = r#"query($org:String!, $user:String!) {
    organization(login: $org) {
        samlIdentityProvider {
            ssoUrl
            externalIdentities(login: $user, first:1) {
                edges {
                    node {
                        guid
                        samlIdentity {
                            nameId
                        }
                        user {
                            login
                        }
                    }
                }
            }
        }
    }
}"#;

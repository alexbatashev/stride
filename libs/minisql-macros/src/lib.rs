use proc_macro::TokenStream;
use proc_macro2::Ident;
use quote::quote;
use syn::{
    LitStr, Result, Token, Type, braced, bracketed,
    parse::{Parse, ParseStream},
    parse_macro_input, token,
};

#[proc_macro]
pub fn migrations(input: TokenStream) -> TokenStream {
    let migrations = parse_macro_input!(input as Migrations);
    let expanded = migrations.expand();
    TokenStream::from(expanded)
}

struct Migrations {
    migrations: Vec<Migration>,
}

struct Migration {
    tables: Vec<Table>,
    raw_sql: Vec<LitStr>,
}

struct Table {
    name: Ident,
    columns: Vec<Column>,
    foreign_keys: Vec<ForeignKey>,
    enable_vectors: bool,
}

struct Column {
    name: Ident,
    column_name: Option<String>,
    ty: Type,
    tags: Vec<SqlTag>,
}

struct ForeignKey {
    fields: Vec<Ident>,
    table: Ident,
    foreign_fields: Vec<Ident>,
}

#[derive(Clone)]
enum SqlTag {
    PrimaryKey,
    Unique,
}

impl Parse for Migrations {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut migrations = Vec::new();

        while !input.is_empty() {
            migrations.push(input.parse()?);
        }

        Ok(Migrations { migrations })
    }
}

impl Parse for Migration {
    fn parse(input: ParseStream) -> Result<Self> {
        let _name: Ident = input.parse()?;
        let content;
        braced!(content in input);

        let mut tables = Vec::new();
        let mut raw_sql = Vec::new();

        while !content.is_empty() {
            let lookahead = content.lookahead1();
            if lookahead.peek(syn::Ident) {
                let ident: Ident = content.fork().parse()?;
                if ident == "table" {
                    tables.push(content.parse()?);
                } else if ident == "raw" {
                    content.parse::<Ident>()?; // consume "raw"
                    let sql: LitStr = content.parse()?;
                    content.parse::<Token![;]>()?;
                    raw_sql.push(sql);
                } else {
                    return Err(lookahead.error());
                }
            } else {
                return Err(lookahead.error());
            }
        }

        Ok(Migration { tables, raw_sql })
    }
}

impl Parse for Table {
    fn parse(input: ParseStream) -> Result<Self> {
        let table_keyword: Ident = input.parse()?;
        if table_keyword != "table" {
            return Err(syn::Error::new(table_keyword.span(), "Expected 'table'"));
        }
        let name: Ident = input.parse()?;
        let content;
        braced!(content in input);

        let mut columns = Vec::new();
        let mut foreign_keys = Vec::new();
        let mut enable_vectors = false;

        while !content.is_empty() {
            let lookahead = content.lookahead1();
            if lookahead.peek(syn::Ident) {
                let ident: Ident = content.fork().parse()?;
                if ident == "foreign_key" {
                    foreign_keys.push(content.parse()?);
                } else if ident == "enable_vectors" {
                    content.parse::<Ident>()?; // consume "enable_vectors"
                    content.parse::<Token![;]>()?;
                    enable_vectors = true;
                } else if content.peek2(Token![:]) {
                    columns.push(content.parse()?);
                } else {
                    return Err(lookahead.error());
                }
            } else {
                return Err(lookahead.error());
            }
        }

        Ok(Table {
            name,
            columns,
            foreign_keys,
            enable_vectors,
        })
    }
}

impl Parse for Column {
    fn parse(input: ParseStream) -> Result<Self> {
        let name: Ident = input.parse()?;
        input.parse::<Token![:]>()?;
        let ty: Type = input.parse()?;

        let mut tags = Vec::new();
        let mut column_name = None;

        // Parse attributes (tags and column name)
        if input.peek(token::Bracket) {
            let content;
            bracketed!(content in input);

            while !content.is_empty() {
                if content.peek(syn::Ident) && content.peek2(Token![=]) {
                    // Parse column_name = "actual_name"
                    let attr_name: Ident = content.parse()?;
                    content.parse::<Token![=]>()?;
                    let attr_value: LitStr = content.parse()?;

                    if attr_name == "column" {
                        column_name = Some(attr_value.value());
                    }

                    if content.peek(Token![,]) {
                        content.parse::<Token![,]>()?;
                    }
                } else {
                    // Parse regular tags
                    let tag_ident: Ident = content.parse()?;
                    let tag = match tag_ident.to_string().as_str() {
                        "PrimaryKey" => SqlTag::PrimaryKey,
                        "Unique" => SqlTag::Unique,
                        _ => return Err(syn::Error::new(tag_ident.span(), "Unknown SQL tag")),
                    };
                    tags.push(tag);

                    if content.peek(Token![,]) {
                        content.parse::<Token![,]>()?;
                    }
                }
            }
        }

        input.parse::<Token![,]>()?;

        Ok(Column {
            name,
            column_name,
            ty,
            tags,
        })
    }
}

impl Parse for ForeignKey {
    fn parse(input: ParseStream) -> Result<Self> {
        let foreign_key_keyword: Ident = input.parse()?;
        if foreign_key_keyword != "foreign_key" {
            return Err(syn::Error::new(
                foreign_key_keyword.span(),
                "Expected 'foreign_key'",
            ));
        }
        let content;
        syn::parenthesized!(content in input);

        // Parse single field or field list
        let first_field: Ident = content.parse()?;
        let mut fields = vec![first_field];

        // Check for additional fields (comma-separated)
        while content.peek(Token![,]) && !content.peek2(Token![-]) {
            content.parse::<Token![,]>()?;
            fields.push(content.parse()?);
        }

        content.parse::<Token![-]>()?;
        content.parse::<Token![>]>()?;

        let table: Ident = content.parse()?;
        content.parse::<Token![.]>()?;

        // Parse single foreign field or field list
        let first_foreign_field: Ident = content.parse()?;
        let mut foreign_fields = vec![first_foreign_field];

        // Check for additional foreign fields (comma-separated)
        while content.peek(Token![,]) {
            content.parse::<Token![,]>()?;
            foreign_fields.push(content.parse()?);
        }

        input.parse::<Token![;]>()?;

        Ok(ForeignKey {
            fields,
            table,
            foreign_fields,
        })
    }
}

impl Migrations {
    fn expand(&self) -> proc_macro2::TokenStream {
        let migration_exprs: Vec<proc_macro2::TokenStream> = self
            .migrations
            .iter()
            .map(|migration| migration.expand())
            .collect();

        // Generate per-table modules for a simple, single-table select DSL
        let table_modules: Vec<proc_macro2::TokenStream> = self
            .migrations
            .iter()
            .flat_map(|m| m.tables.iter())
            .map(|t| t.expand_module())
            .collect();

        quote! {
            pub fn get_migrations() -> Vec<::minisql::Migration> {
                vec![
                    #(#migration_exprs),*
                ]
            }

            #(#table_modules)*
        }
    }
}

impl Migration {
    fn expand(&self) -> proc_macro2::TokenStream {
        let table_exprs: Vec<proc_macro2::TokenStream> =
            self.tables.iter().map(|table| table.expand()).collect();

        let raw_sql_exprs: Vec<proc_macro2::TokenStream> = self
            .raw_sql
            .iter()
            .map(|sql| {
                quote! { .raw(#sql) }
            })
            .collect();

        quote! {
            ::minisql::Migration::new()
                #(.table(#table_exprs))*
                #(#raw_sql_exprs)*
        }
    }
}

impl Table {
    fn expand(&self) -> proc_macro2::TokenStream {
        let table_name = self.name.to_string();

        let column_exprs: Vec<proc_macro2::TokenStream> =
            self.columns.iter().map(|column| column.expand()).collect();

        let foreign_key_exprs: Vec<proc_macro2::TokenStream> =
            self.foreign_keys.iter().map(|fk| fk.expand()).collect();

        let enable_vectors = if self.enable_vectors {
            quote! { .enable_vectors() }
        } else {
            quote! {}
        };

        quote! {
            ::minisql::Table::from_name(#table_name)
                #(#column_exprs)*
                #(#foreign_key_exprs)*
                #enable_vectors
        }
    }

    // Emit a table module with typed Row, Column consts, and select helpers
    fn expand_module(&self) -> proc_macro2::TokenStream {
        let mod_ident = &self.name;
        let table_name_str = self.name.to_string();

        // fields for Row
        let row_fields: Vec<proc_macro2::TokenStream> = self
            .columns
            .iter()
            .map(|c| {
                let name_ident = &c.name;
                let ty = &c.ty;
                quote! { pub #name_ident: #ty }
            })
            .collect();

        // Column consts (DB column name may differ via [column = "..."])
        let column_consts: Vec<proc_macro2::TokenStream> = self
            .columns
            .iter()
            .map(|c| {
                let name_ident = &c.name;
                let ty = &c.ty;
                let db_name = c
                    .column_name
                    .clone()
                    .unwrap_or_else(|| c.name.to_string());
                quote! { pub const #name_ident: ::minisql::Column<#ty, Table> = ::minisql::Column::new(#db_name); }
            })
            .collect();

        // Row construction from generic Row
        let row_inits: Vec<proc_macro2::TokenStream> = self
            .columns
            .iter()
            .map(|c| {
                let name_ident = &c.name;
                let ty = &c.ty;
                let db_name = c.column_name.clone().unwrap_or_else(|| c.name.to_string());
                quote! { #name_ident: r.decode::<#ty>(#db_name)? }
            })
            .collect();

        // Insert builder setters per column
        let insert_setters: Vec<proc_macro2::TokenStream> = self
            .columns
            .iter()
            .map(|c| {
                let name_ident = &c.name;
                let ty = &c.ty;
                let db_name = c.column_name.clone().unwrap_or_else(|| c.name.to_string());
                quote! {
                    pub fn #name_ident<U: ::minisql::ColumnInput<#ty>>(mut self, v: U) -> Self {
                        self.cols.push(#db_name);
                        let val = <U as ::minisql::ColumnInput<#ty>>::into_value(v);
                        self.params.push(val);
                        self
                    }
                }
            })
            .collect();

        quote! {
            pub mod #mod_ident {
                use super::*;
                /// Marker type for this table
                pub struct Table;

                /// Strongly-typed row for this table
                pub struct Row { #( #row_fields, )* }

                #( #column_consts )*

                /// Local wrapper over minisql::Select to allow inherent impls
                pub struct SelectQuery(pub(crate) ::minisql::Select<Table>);

                /// Start a SELECT * query for this table
                pub fn select() -> SelectQuery {
                    SelectQuery(::minisql::Select::new(#table_name_str))
                }

                impl SelectQuery {
                    pub fn where_(self, predicate: ::minisql::Expr<Table>) -> Self {
                        SelectQuery(self.0.where_(predicate))
                    }

                    pub fn limit(self, n: u64) -> Self {
                        SelectQuery(self.0.limit(n))
                    }

                    pub fn offset(self, n: u64) -> Self {
                        SelectQuery(self.0.offset(n))
                    }

                    pub fn order_by_asc<T>(mut self, column: ::minisql::Column<T, Table>) -> Self {
                        self.0.order_by(column, true);
                        self
                    }

                    pub fn order_by_desc<T>(mut self, column: ::minisql::Column<T, Table>) -> Self {
                        self.0.order_by(column, false);
                        self
                    }

                    /// Fetch all rows for this select
                    pub async fn all(self, db: &::minisql::ConnectionPool) -> Result<Vec<Row>, Box<dyn std::error::Error + Send + Sync>> {
                        let (sql, params) = self.0.to_sql();
                        let res = db.query_with_params(&sql, params).await?;
                        let mut out = Vec::with_capacity(res.rows().len());
                        for r in res.rows() {
                            let row = Row { #( #row_inits, )* };
                            out.push(row);
                        }
                        Ok(out)
                    }

                    /// Fetch exactly one row or return an error
                    pub async fn one(self, db: &::minisql::ConnectionPool) -> Result<Row, Box<dyn std::error::Error + Send + Sync>> {
                        let mut rows = self.limit(1).all(db).await?;
                        rows.pop().ok_or_else(|| "query returned 0 rows".into())
                    }
                }

                /// Local wrapper for projection selects
                pub struct SelectColsQuery<C: ::minisql::SelectList<Table>>(pub(crate) ::minisql::SelectCols<Table, C>);

                /// Start a SELECT of specific columns
                pub fn select_cols<C: ::minisql::SelectList<Table>>(cols: C) -> SelectColsQuery<C> {
                    SelectColsQuery(::minisql::Select::new(#table_name_str).select(cols))
                }

                impl<C: ::minisql::SelectList<Table>> SelectColsQuery<C> {
                    pub fn where_(self, predicate: ::minisql::Expr<Table>) -> Self {
                        SelectColsQuery(self.0.where_(predicate))
                    }
                    pub fn limit(self, n: u64) -> Self {
                        SelectColsQuery(self.0.limit(n))
                    }
                    pub fn offset(self, n: u64) -> Self {
                        SelectColsQuery(self.0.offset(n))
                    }
                    pub fn order_by_asc<T>(self, column: ::minisql::Column<T, Table>) -> Self {
                        SelectColsQuery(self.0.order_by(column, true))
                    }
                    pub fn order_by_desc<T>(self, column: ::minisql::Column<T, Table>) -> Self {
                        SelectColsQuery(self.0.order_by(column, false))
                    }

                    pub async fn all(self, db: &::minisql::ConnectionPool) -> Result<Vec<<C as ::minisql::SelectList<Table>>::Out>, Box<dyn std::error::Error + Send + Sync>> {
                        let (sql, params) = self.0.to_sql();
                        let res = db.query_with_params(&sql, params).await?;
                        let mut out = Vec::with_capacity(res.rows().len());
                        let colnames = self.0.names_slice().to_vec();
                        for r in res.rows() {
                            let row = <C as ::minisql::SelectList<Table>>::decode_row(r, &colnames)?;
                            out.push(row);
                        }
                        Ok(out)
                    }

                    pub async fn one(self, db: &::minisql::ConnectionPool) -> Result<<C as ::minisql::SelectList<Table>>::Out, Box<dyn std::error::Error + Send + Sync>> {
                        let mut rows = self.limit(1).all(db).await?;
                        rows.pop().ok_or_else(|| "query returned 0 rows".into())
                    }
                }

                /// Typed insert builder for this table
                pub struct InsertBuilder {
                    cols: Vec<&'static str>,
                    params: Vec<::minisql::Value>,
                }

                /// Start an INSERT builder for this table
                pub fn insert() -> InsertBuilder {
                    InsertBuilder { cols: Vec::new(), params: Vec::new() }
                }

                impl InsertBuilder {
                    #(#insert_setters)*

                    pub async fn execute(self, db: &::minisql::ConnectionPool) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
                        if self.cols.is_empty() {
                            return Ok(0);
                        }
                        let placeholders = std::iter::repeat("?")
                            .take(self.cols.len())
                            .collect::<Vec<_>>()
                            .join(", ");
                        let sql = format!(
                            "INSERT INTO {} ({}) VALUES ({});",
                            #table_name_str,
                            self.cols.join(", "),
                            placeholders
                        );
                        let res = db.query_with_params(&sql, self.params).await?;
                        Ok(res.affected_rows())
                    }
                }
            }
        }
    }
}

impl Column {
    fn expand(&self) -> proc_macro2::TokenStream {
        let name_string = self.name.to_string();
        let column_name = self.column_name.as_ref().unwrap_or(&name_string);
        let column_type = &self.ty;

        let tags: Vec<proc_macro2::TokenStream> = self
            .tags
            .iter()
            .map(|tag| match tag {
                SqlTag::PrimaryKey => quote! { ::minisql::SqlTag::PrimaryKey },
                SqlTag::Unique => quote! { ::minisql::SqlTag::Unique },
            })
            .collect();

        quote! {
            .add::<#column_type, _>(#column_name, &[#(#tags),*])
        }
    }
}

impl ForeignKey {
    fn expand(&self) -> proc_macro2::TokenStream {
        let fields: Vec<String> = self.fields.iter().map(|f| f.to_string()).collect();
        let table_name = self.table.to_string();
        let foreign_fields: Vec<String> =
            self.foreign_fields.iter().map(|f| f.to_string()).collect();

        quote! {
            .foreign_key(&[#(#fields),*], #table_name, &[#(#foreign_fields),*])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_macro_simple() {
        let input = quote::quote! {
            test_migration {
                table users {
                    id: String,
                    name: String,
                }
            }
        };

        let result = std::panic::catch_unwind(|| {
            let parsed: Migrations = syn::parse2(input).unwrap();
            parsed.expand()
        });

        assert!(result.is_ok());
    }
}

use oracle::{self, ValueProjector};
use crate::{metainfo as mi, datasource};

pub struct DynamicQuery {
    table_name:    String,
    columns:       Vec<ColTypeInfo>,
    column_names:  Vec<String>,

    param_columns:      Vec<ColTypeInfo>,
    param_column_names: Vec<String>,
    parsed_params:      Vec<ParsedParameter>
}

struct DynamicResultsProvider {
    column_names: Vec<String>,
    columns:      Vec<ColTypeInfo>
}

struct DynamicParamsProvider {
    columns: Vec<ColTypeInfo>
}

enum ParsedParameter {
    Int16 (i16), Int32(i32), Int64(i64), Varchar(String)
}

struct ColTypeInfo {
    pub col_type:      oracle::SqlType,
    pub oci_data_type: oracle::TypeDescriptor,
}

impl ColTypeInfo {
    fn new(info: &mi::ColumnInfo) -> ColTypeInfo {
        ColTypeInfo { col_type: info.col_type, oci_data_type: info.oci_data_type }
    }
}

impl DynamicQuery {
    pub fn create_from_pk(schema_name: &str, table_info: &mi::TableInfo, parameter: String) -> Result<DynamicQuery, &'static str> {
        match &table_info.primary_key {
            None => Err("Primary key not exists"),
            Some(pk) => {
                let pk_indices = &pk.column_indices;
                if pk_indices.len() > 1 {
                    return Err("Primary key must have only ONE column")
                }
                let pk_column_index = unsafe { pk_indices.get_unchecked(0) };
                let pk_column = unsafe { table_info.columns.get_unchecked(*pk_column_index) };

                let columns: Vec<ColTypeInfo> = table_info.columns.iter().map(ColTypeInfo::new).collect();
                let column_names: Vec<&str> = table_info.columns.iter().map(|c|c.name.as_str()).collect();

                let param_column_names = vec![pk_column.name.clone()];
                let pk_column = ColTypeInfo::new( pk_column );

                let table_name = format!("{}.{}", schema_name, table_info.name.as_str());
                let column_names = column_names.iter().map(|name|name.to_string()).collect();

                ParsedParameter::parse(pk_column.col_type, parameter)
                    .map(|parsed_parameter|DynamicQuery{table_name, columns, column_names, param_columns: vec![pk_column], param_column_names, parsed_params: vec![parsed_parameter]})
            }
        }
    }

    pub fn create_from_params(schema_name: &str, table_info: &mi::TableInfo, parameters: Vec<(String,String)>) -> Result<DynamicQuery, String> {
        let columns: Vec<ColTypeInfo> = table_info.columns.iter().map(ColTypeInfo::new).collect();
        let column_names: Vec<&str> = table_info.columns.iter().map(|c|c.name.as_str()).collect();

        let param_columns_len = parameters.len();

        let mut param_column_names = Vec::with_capacity(param_columns_len);
        let mut param_columns = Vec::with_capacity(param_columns_len);
        let mut parsed_params = Vec::with_capacity(param_columns_len);

        for (ref col_name,ref p) in parameters {
            let column = table_info.columns.iter().find(|c|&c.name == col_name);

            match column {
                None => return Err(format!("Not found column {}", col_name)),
                Some(column) => {
                    let parsed = ParsedParameter::parse(column.col_type, p.to_string());
                    match parsed {
                        Err(err) => return Err(format!("Can not parse parameter value {} for column {}: {}", p, col_name, err)),
                        Ok(parsed) => {
                            parsed_params.push(parsed);
                            param_columns.push(ColTypeInfo::new( column ));
                            param_column_names.push(col_name.to_owned());
                        }
                    }
                }
            }
        }

        let table_name = format!("{}.{}", schema_name, table_info.name.as_str());
        let column_names = column_names.iter().map(|name|name.to_string()).collect();

        Ok( DynamicQuery { table_name, columns, column_names, param_columns, param_column_names, parsed_params } )
    }

    fn generate_sql(&self) -> String {
        let joined_result_columns = self.column_names.join(",");

        let enumerated_param_columns: Vec<String> =
            self.param_column_names.iter().enumerate().map(|(idx,name)|format!("{} = :{}", name, idx+1)).collect();
        let joined_param_columns = enumerated_param_columns.join(" AND ");

        format!("SELECT {} FROM {} WHERE {}", joined_result_columns, self.table_name, joined_param_columns)
    }

    /// execute a query and generate JSON result
    pub fn fetch_one(self) -> Result<String,String> {
        let conn = datasource::get_connection()
            .map_err(|err|format!("Can not connect to oracle: {}", err))?;

        let (query, params) = self.prepare_query(&conn)?;

        let result = query.fetch_one(params)
            .map_err(|err|format!("Can not fetch row by pk: {}", err))?;

        Ok( result.map_or("{}".to_string(), |r| format!("{}", r))  )
    }

    /// execute a query and generate JSON result
    pub fn fetch_many(self) -> Result<String,String> {
        let conn = datasource::get_connection()
            .map_err(|err|format!("Can not connect to oracle: {}", err))?;

        let (query, params) = self.prepare_query(&conn)?;

        let result = query.fetch_list(params)
            .map_err(|err|format!("Can not fetch row by where clause: {}", err))?.join(",");

        Ok( format!("[{}]", result) )
    }

    fn prepare_query<'conn>(self, conn: &'conn oracle::Connection) -> Result<(oracle::Query<'conn, Vec<ParsedParameter>, String>, Vec<ParsedParameter>), String> {
        let sql = self.generate_sql();
        let results_provider = Box::new( DynamicResultsProvider { columns: self.columns, column_names: self.column_names } );
        let params_provider = Box::new( DynamicParamsProvider { columns: self.param_columns });

        let stmt = conn.prepare_dynamic(&sql, params_provider)
            .map_err(|err|format!("Can not prepare statement: {}", err))?;

        let query = stmt.query_dynamic(results_provider, 1)
            .map_err(|err|format!("Can not create query from statement: {}", err))?;

        Ok((query, self.parsed_params))
    }

}

impl ParsedParameter {
    fn parse(tp: oracle::SqlType, value: String) -> Result<Self, &'static str> {
        match tp {
            oracle::SqlType::Int16 => {
                let val: i16 = value.parse().unwrap();
                Ok(ParsedParameter::Int16(val))
            },
            oracle::SqlType::Int32 => {
                let val: i32 = value.parse().unwrap();
                Ok(ParsedParameter::Int32(val))
            },
            oracle::SqlType::Int64 => {
                let val: i64 = value.parse().unwrap();
                Ok(ParsedParameter::Int64(val))
            },
            oracle::SqlType::Varchar => {
                Ok(ParsedParameter::Varchar(value))
            },
            _ => Err("Not supported type for Primary key")
        }
    }

    fn project_value(&self, p: &mut oracle::ParamValue) {
        match self {
            Self::Int16(val) => {
                val.project_value(p);
            },
            Self::Int32(val) => {
                val.project_value(p);
            },
            Self::Int64(val) => {
                val.project_value(p);
            },
            Self::Varchar(val) => {
                val.project_value(p);
            },
        };
    }
}

impl oracle::ResultsProvider<String> for DynamicResultsProvider {
    fn sql_descriptors(&self) -> Vec<oracle::TypeDescriptor> {
        self.columns.iter().map(|c|c.oci_data_type.clone()).collect()
    }

    fn gen_result(&self, rs: oracle::ResultSet) -> String {
        let results: Vec<String> = self.columns
            .iter()
            .zip(self.column_names.iter())
            .zip(rs.iter())
            .map(|((c, name), value)|{
                let result = value.to_owned().try_to_string(&c.col_type).unwrap_or_else(|err| err.to_string());
                format!("\"{}\":{}", name, result)
            }).collect();

        format!("{{ {} }}", results.join(","))
    }
}

impl oracle::ParamsProvider<Vec<ParsedParameter>> for DynamicParamsProvider {
    fn members(&self) -> Vec<oracle::Member> {
        self.columns.iter()
            .map(|c| {
                oracle::Member::new(c.oci_data_type, oracle::Identifier::Unnamed)
            })
            .collect()
    }

    fn project_values(&self, params: &Vec<ParsedParameter>, projecton: &mut oracle::ParamsProjection) {
        for (idx,param) in params.iter().enumerate() {
            let p = unsafe { projecton.get_unchecked_mut(idx) };
            param.project_value(p);
        }
    }
}

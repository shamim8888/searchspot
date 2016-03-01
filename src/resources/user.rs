use chrono::UTC;
use chrono::datetime::DateTime;

use params::*;

use rs_es::Client;
use rs_es::query::{Filter, Query};
use rs_es::units::JsonVal;
use rs_es::operations::search::{Sort, SortField, Order};

use terms::VectorOfTerms;

#[derive(Debug, RustcDecodable)]
struct TalentsSearchResult {
  id: u32
}

pub struct Talent;

impl Talent {
  /// Return a `Vec<Filter>` with visibility criteria for the talents.
  /// The `epoch` must be given as `I64` (UNIX time in seconds) and is
  /// the range in which batches are searched.
  /// If `presented_talents` is provided, talents who match the IDs
  /// contained there skip the standard visibility criteria.
  ///
  /// Basically, the talents must be accepted into the platform and must be
  /// inside a living batch to match the visibility criteria.
  fn visibility_filters(epoch: i64, presented_talents: Vec<i32>) -> Vec<Filter> {
    let visibility_rules = Filter::build_bool()
                                  .with_must(
                                    vec![
                                      Filter::build_term("accepted", true)
                                             .build(),
                                      Filter::build_range("batch_start_at")
                                             .with_lte(JsonVal::from(epoch))
                                             .with_format("epoch_second")
                                             .build(),
                                      Filter::build_range("batch_end_at")
                                             .with_gte(JsonVal::from(epoch))
                                             .with_format("epoch_second")
                                             .build()
                                    ])
                                  .build();

    if presented_talents.len() > 0 { // preferred over !_.is_empty()
      let presented_talents_filters = Filter::build_bool()
                                             .with_must(
                                               vec![
                                                 <Filter as VectorOfTerms<i32>>::build_terms(
                                                   "ids", &presented_talents)
                                               ].into_iter()
                                                .flat_map(|x| x)
                                                .collect::<Vec<Filter>>())
                                             .build();
      vec![
        Filter::build_bool()
               .with_should(vec![visibility_rules, presented_talents_filters])
               .build()
      ]
    }
    else {
      vec![visibility_rules]
    }
  }

  /// Given parameters inside the query string mapped inside a `Map`,
  /// and the `epoch` (defined as UNIX time in seconds) for batches,
  /// return a `Query` for ElasticSearch.
  ///
  /// `VectorOfTerms` are ORred, while `Filter`s are ANDed.
  /// I.e.: given ["Fullstack", "DevOps"] as `work_roles`, found talents
  /// will present at least one of these roles), but both `work_roles`
  /// and `work_languages`, if provided, must not be empty.
  fn search_filters(params: &Map, epoch: i64) -> Query {
    let company_id = i32_vec_from_params!(params, "company_id");

    Query::build_filtered(Filter::build_bool()
                                 .with_must(
                                   vec![
                                     <Filter as VectorOfTerms<String>>::build_terms(
                                       "work_roles", &vec_from_params!(params, "work_roles")),

                                     <Filter as VectorOfTerms<String>>::build_terms(
                                       "work_languages", &vec_from_params!(params, "work_languages")),

                                     <Filter as VectorOfTerms<String>>::build_terms(
                                       "work_experience", &vec_from_params!(params, "work_experience")),

                                     <Filter as VectorOfTerms<String>>::build_terms(
                                       "work_locations", &vec_from_params!(params, "work_locations")),

                                     <Filter as VectorOfTerms<String>>::build_terms(
                                      "work_authorization", &vec_from_params!(params, "work_authorization")),

                                     Talent::visibility_filters(epoch,
                                       i32_vec_from_params!(params, "presented_talents"))
                                   ].into_iter()
                                    .flat_map(|x| x)
                                    .collect::<Vec<Filter>>())
                                 .with_must_not(
                                   vec![
                                     <Filter as VectorOfTerms<i32>>::build_terms(
                                       "company_ids", &company_id),

                                     <Filter as VectorOfTerms<i32>>::build_terms(
                                       "blocked_companies", &company_id)
                                   ].into_iter()
                                    .flat_map(|x| x)
                                    .collect::<Vec<Filter>>())
                                 .build())
          .build()
  }

  /// Query ElasticSearch on given `indexes` and `params` and return the IDs of
  /// the found talents.
  pub fn search(mut es: &mut Client, default_indexes: &[&str], params: &Map) -> Vec<u32> {
    let epoch = match params.find(&["epoch"]) {
      Some(&Value::I64(epoch)) => epoch,
      _ => DateTime::timestamp(&UTC::now())
    };

    let indexes: Vec<&str> = match params.find(&["index"]) {
      Some(&Value::String(ref index)) => vec![&index[..]],
      _ => default_indexes.to_vec()
    };

    let result = es.search_query()
                   .with_indexes(&indexes)
                   .with_query(&Talent::search_filters(params, epoch))
                   .with_sort(&Talent::sorting_criteria())
                   .send();

    match result {
      Ok(result) => {
        result.hits.hits.into_iter()
                        .map(|hit| {
                          let talent: TalentsSearchResult = hit.source().unwrap();
                          talent.id
                        })
                        .collect::<Vec<u32>>()
      },
      Err(err) => {
        println!("{:?}", err);
        vec![]
      }
    }
  }

  /// Return a `Sort` that makes values be sorted for `updated_at`, descendently.
  fn sorting_criteria() -> Sort {
    Sort::new(
      vec![
        SortField::new("updated_at", Some(Order::Desc)).build()
      ])
  }
}

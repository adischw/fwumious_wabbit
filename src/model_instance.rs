use std::error::Error;
use std::io::Error as IOError;
use std::io::ErrorKind;

use std::io::Read;
use std::fs::File;
use serde::{Serialize,Deserialize};//, Deserialize};
use serde_json::{Value};

use crate::vwmap;
use crate::consts;
use crate::feature_transform_parser;


#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct FeatureComboDesc {
    pub feature_indices: Vec<u32>,
    pub weight:f32,
}


#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Copy)]
pub enum Optimizer {
    SGD = 1,
    Adagrad = 2,
}


#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModelInstance {
    pub learning_rate: f32,    
    #[serde(default = "default_f32_zero")]
    pub minimum_learning_rate: f32,
    pub power_t: f32,
    pub bit_precision: u8,
    pub hash_mask: u32,		// DEPRECATED, UNUSED -- this is recalculated in feature_buffer.rs
    pub add_constant_feature: bool,
    pub feature_combo_descs: Vec<FeatureComboDesc>,
    pub ffm_fields: Vec<Vec<u32>>,
    #[serde(default = "default_u32_zero")]
    pub ffm_k: u32,
    #[serde(default = "default_u32_zero")]
    pub ffm_bit_precision: u32,
    #[serde(default = "default_bool_false")]
    pub ffm_separate_vectors: bool, // DEPRECATED, UNUSED
    #[serde(default = "default_bool_false")]
    pub fastmath: bool,

    #[serde(default = "default_f32_zero")]
    pub ffm_k_threshold: f32,
    #[serde(default = "default_f32_zero")]
    pub ffm_init_center: f32,
    #[serde(default = "default_f32_zero")]
    pub ffm_init_width: f32,
    #[serde(default = "default_f32_zero")]
    pub ffm_init_zero_band: f32,	// from 0.0 to 1.0, percentage of ffm_init_width
    #[serde(default = "default_f32_zero")]
    pub ffm_init_acc_gradient: f32,
    #[serde(default = "default_f32_zero")]
    pub init_acc_gradient: f32,
    // these are only used for learning, so it doesnt matter they got set to zero as default        
    #[serde(default = "default_f32_zero")]
    pub ffm_learning_rate: f32,    
    #[serde(default = "default_f32_zero")]
    pub ffm_power_t: f32,

    #[serde(default = "default_optimizer_adagrad")]
    pub optimizer: Optimizer,
    
    pub transform_namespaces: feature_transform_parser::NamespaceTransforms,
    
}

fn default_u32_zero() -> u32{0}
fn default_f32_zero() -> f32{0.0}
fn default_bool_false() -> bool{false}
fn default_optimizer_adagrad() -> Optimizer{Optimizer::Adagrad}


pub fn get_float_namespaces<'a>(cl: &clap::ArgMatches<'a>) -> Result<(String, u32), Box<dyn Error>> {
   if let Some(in_v) = cl.value_of("float_namespaces") {
       let prefix_skip:u32 = match cl.value_of("float_namespaces_skip_prefix") {
           Some(prefix_skip_str) => prefix_skip_str.parse()?,
           None => 0,
       };
       Ok((in_v.to_owned(), prefix_skip))
   } else {
       Ok(("".to_owned(), 0))
   }
}


impl ModelInstance {
    pub fn new_empty() -> Result<ModelInstance, Box<dyn Error>> {
        let mi = ModelInstance {
            learning_rate: 0.5, // vw default
            ffm_learning_rate: 0.5, // vw default
            minimum_learning_rate: 0.0, 
            bit_precision: 18,      // vw default
            hash_mask: 0, // DEPRECATED, UNUSED
            power_t: 0.5,
            ffm_power_t: 0.5,
            add_constant_feature: true,
            feature_combo_descs: Vec::new(),
            ffm_fields: Vec::new(),
            ffm_k: 0,
            ffm_bit_precision: 18,
            ffm_separate_vectors: false, // DEPRECATED, UNUSED
            fastmath: true,
            ffm_k_threshold: 0.0,
            ffm_init_center: 0.0,
            ffm_init_width: 0.0,
            ffm_init_zero_band: 0.0,
            ffm_init_acc_gradient: 0.0,
            init_acc_gradient: 1.0,
            optimizer: Optimizer::SGD,
            transform_namespaces: feature_transform_parser::NamespaceTransforms::new(),
        };
        Ok(mi)
    }

    
    pub fn create_feature_combo_desc(&self, vw: &vwmap::VwNamespaceMap, s: &str) -> Result<FeatureComboDesc, Box<dyn Error>> {

        let vsplit: Vec<&str> = s.split(":").collect(); // We use : as a delimiter for weight
        let mut combo_weight: f32 = 1.0;
        if vsplit.len() > 2 {
            return Err(Box::new(IOError::new(ErrorKind::Other, format!("only one value parameter allowed (denoted with \":\"): \"{:?}\"", s))))
        }
        if vsplit.len() == 2 {
            let weight_str = vsplit[1];
            combo_weight = weight_str.parse()?;
        }

        let namespaces_str = vsplit[0];
        let mut feature_indices: Vec<u32> = Vec::new();
        for char in namespaces_str.chars() {
           // create an list of indexes dfrom list of namespace chars
           
           let index = feature_transform_parser::get_namespace_id(&self.transform_namespaces, vw, char)?;
           feature_indices.push(index);
        }
        Ok(FeatureComboDesc {
                             feature_indices: feature_indices,
                              weight: combo_weight
                            })
    }


    
    pub fn new_from_cmdline<'a>(cl: &clap::ArgMatches<'a>, vw: &vwmap::VwNamespaceMap) -> Result<ModelInstance, Box<dyn Error>> {
        let mut mi = ModelInstance::new_empty()?;

        let vwcompat: bool = cl.is_present("vwcompat");
        
        if vwcompat {
            mi.fastmath = false;

            mi.init_acc_gradient = 0.0;

            if !cl.is_present("keep") {
                return Err(Box::new(IOError::new(ErrorKind::Other, "--vwcompat requires at least one --keep parameter, we do not implicitly take all features available")))
            }

            // Vowpal supports a mode with "prehashed" features, where numeric strings are treated as
            // numeric precomputed hashes. This is even default option.
            // It is generally a bad idea except if you strings really are precomputed hashes... 
            if !cl.is_present("hash") {
                   return Err(Box::new(IOError::new(ErrorKind::Other, format!("--vwcompat requires use of --hash all"))))
            } else

            if let Some(val) = cl.value_of("hash") {
                if val != "all" {
                    return Err(Box::new(IOError::new(ErrorKind::Other, format!("--vwcompat requires use of --hash all"))))
                }            
            }

            // --sgd will turn off adaptive, invariant and normalization in vowpal. You can turn adaptive back on in vw and fw with --adaptive
            if !cl.is_present("sgd") {
                return Err(Box::new(IOError::new(ErrorKind::Other, format!("--vwcompat requires use of --sgd"))))
            }
            
        
        }

        // we first need transform namespaces, before processing keep or interactions
        if let Some(in_v) = cl.values_of("transform_namespace") {
            for value_str in in_v {                
                mi.transform_namespaces.add_transform_namespace(vw, value_str)?;
            }
        }
        
        if let Some(in_v) = cl.values_of("keep") {
            for value_str in in_v {
                mi.feature_combo_descs.push(mi.create_feature_combo_desc(vw, value_str)?);
            }
        }
        
        if let Some(in_v) = cl.values_of("interactions") {
            for value_str in in_v {                
                mi.feature_combo_descs.push(mi.create_feature_combo_desc(vw, value_str)?);
            }
        }



        if let Some(in_v) = cl.value_of("lrqfa") {
            let vsplit: Vec<&str> = in_v.split("-").collect(); // We use - as a delimiter instead of first numbers as vowpal does it
            if vsplit.len() != 2 {
                return Err(Box::new(IOError::new(ErrorKind::Other, format!("--lrqfa takes namespaces-k, example: \"ABC-12\", your string was: \"{}\"", in_v))))
            }
            let namespaces_str = vsplit[0];
            let k_str = vsplit[1];
            for char in namespaces_str.chars() {
                // create an list of indexes dfrom list of namespace chars
                let index = feature_transform_parser::get_namespace_id(&mi.transform_namespaces, vw, char)?;
                mi.ffm_fields.push(vec![index]);
            }
            mi.ffm_k = k_str.parse().expect("Number expected");
            if mi.ffm_k > consts::FFM_MAX_K as u32{
                return Err(Box::new(IOError::new(ErrorKind::Other, format!("Maximum ffm_k is: {}, passed: {}", consts::FFM_MAX_K, mi.ffm_k))))
            }
        }

        if let Some(val) = cl.value_of("ffm_k") {
            mi.ffm_k = val.parse()?;
            if mi.ffm_k > consts::FFM_MAX_K as u32{
                return Err(Box::new(IOError::new(ErrorKind::Other, format!("Maximum ffm_k is: {}, passed: {}", consts::FFM_MAX_K, mi.ffm_k))))
            }
        }        

        if let Some(val) = cl.value_of("ffm_init_center") {
            mi.ffm_init_center = val.parse()?;
        }

        if let Some(val) = cl.value_of("ffm_init_width") {
            mi.ffm_init_width = val.parse()?;
        }

        if let Some(val) = cl.value_of("init_acc_gradient") {
            if vwcompat {
                return Err(Box::new(IOError::new(ErrorKind::Other, "Initial accumulated gradient is not supported in --vwcompat mode")))
            }
            mi.init_acc_gradient = val.parse()?;
        }
        
        if let Some(val) = cl.value_of("ffm_init_acc_gradient") {
            mi.ffm_init_acc_gradient = val.parse()?;
        } else {
            mi.ffm_init_acc_gradient = mi.init_acc_gradient;
        }

        if let Some(in_v) = cl.values_of("ffm_field") {
            for namespaces_str in in_v {          
                let mut field: Vec<u32>= Vec::new();
                for char in namespaces_str.chars() {
                    //println!("K: {}", char);
                    let index = feature_transform_parser::get_namespace_id(&mi.transform_namespaces, vw, char)?;
                    field.push(index);
                }
                mi.ffm_fields.push(field);
            }
        }
        
        if let Some(val) = cl.value_of("ffm_bit_precision") {
            mi.ffm_bit_precision = val.parse()?;
            println!("FFM num weight bits = {}", mi.ffm_bit_precision); // vwcompat
        }

        if let Some(val) = cl.value_of("bit_precision") {
            mi.bit_precision = val.parse()?;
            println!("Num weight bits = {}", mi.bit_precision); // vwcompat
        }

        if let Some(val) = cl.value_of("learning_rate") {
            mi.learning_rate = val.parse()?;
        }
        if let Some(val) = cl.value_of("ffm_learning_rate") {
            mi.ffm_learning_rate = val.parse()?;
        } else {
            mi.ffm_learning_rate = mi.learning_rate;
        }




        if let Some(val) = cl.value_of("minimum_learning_rate") {
            mi.minimum_learning_rate = val.parse()?;
        }

        if let Some(val) = cl.value_of("power_t") {
            mi.power_t = val.parse()?;
        }
        if let Some(val) = cl.value_of("ffm_power_t") {
            mi.ffm_power_t = val.parse()?;
        } else {
            mi.ffm_power_t = mi.power_t;
        }
        
        if let Some(val) = cl.value_of("link") {
            if val != "logistic" {
                return Err(Box::new(IOError::new(ErrorKind::Other, format!("--link only supports 'logistic'"))))
            }            
        }
        if let Some(val) = cl.value_of("loss_function") {
            if val != "logistic" {
                return Err(Box::new(IOError::new(ErrorKind::Other, format!("--loss_function only supports 'logistic'"))))
            }            
        }
        if let Some(val) = cl.value_of("l2") {
            let v2:f32 = val.parse()?;
            if v2.abs() > 0.00000001 {
                return Err(Box::new(IOError::new(ErrorKind::Other, format!("--l2 can only be 0.0"))))
            }
        }

        if cl.is_present("noconstant") {
            mi.add_constant_feature = false;
        }

        // We currently only support SGD + adaptive, which means both options have to be specified
        if cl.is_present("sgd") {
            mi.optimizer = Optimizer::SGD;
        }

        if cl.is_present("adaptive") {
            mi.optimizer = Optimizer::Adagrad;
        }

        
        
        Ok(mi)
    }

/*
    pub fn new_from_jsonfile(input_filename: &str, vw: &vwmap::VwNamespaceMap) -> Result<ModelInstance, Box<dyn Error>> {
        let mut mi = ModelInstance::new_empty()?;
        let mut input = File::open(input_filename)?;
        let mut contents = String::new();
        input.read_to_string(&mut contents)?;
        let j: Value = serde_json::from_str(&contents)?;
        let descj = &j["desc"];
        mi.learning_rate = descj["learning_rate"].as_f64().unwrap() as f32;
        mi.bit_precision = descj["bit_precision"].as_u64().unwrap() as u8;
        let features = descj["features"].as_array().unwrap();
        for feature in features {
            let mut feature_combo_desc = FeatureComboDesc {
                                feature_indices: Vec::new(),
                                weight: 1.0,
                                };

            let fname = feature.as_str().unwrap();
            let primitive_features = fname.split(",");
            for primitive_feature_name in primitive_features {
                let index = match vw.map_name_to_index.get(primitive_feature_name) {
                    Some(index) => *index,
                    None => return Err(Box::new(IOError::new(ErrorKind::Other, format!("Unknown feature name in model json: {}", primitive_feature_name))))
                };
                let index = mi.get_namespace_id(vw, primitive_feature_name)?;

                feature_combo_desc.feature_indices.push(index);
            }
            mi.feature_combo_descs.push(feature_combo_desc);
//            mi.feature_combos.push(feature_vec);
        }

        Ok(mi)
    }
    */
}


#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    #[test]
    fn test_interaction_parsing() {
        let vw_map_string = r#"
A,featureA
B,featureB
C,featureC
"#;
        let vw = vwmap::VwNamespaceMap::new(vw_map_string, ("".to_string(), 0)).unwrap();
        let mi = ModelInstance::new_empty().unwrap();        
        
        let aa = mi.create_feature_combo_desc(&vw, "A").unwrap();
        assert_eq!(aa, FeatureComboDesc {
                                feature_indices: vec![0],
                                weight: 1.0
                                });
        
        let aa = mi.create_feature_combo_desc(&vw, "BA:1.5").unwrap();
        assert_eq!(aa, FeatureComboDesc {
                                feature_indices: vec![1,0],
                                weight: 1.5
                                });
                                
    }

    #[test]
    fn test_weight_parsing() {
        let vw_map_string = r#"
A,featureA:2
B,featureB:3
"#;
        // The main point is that weight in feature names from vw_map_str is ignored
        let vw = vwmap::VwNamespaceMap::new(vw_map_string, ("".to_string(), 0)).unwrap();
        let mi = ModelInstance::new_empty().unwrap();        
        let aa = mi.create_feature_combo_desc(&vw, "BA:1.5").unwrap();
        assert_eq!(aa, FeatureComboDesc {
                                feature_indices: vec![1,0],
                                weight: 1.5
                                });
                                
    }




}








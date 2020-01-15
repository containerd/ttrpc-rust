use cmd_lib::run_cmd;

fn main() {
    let result = run_cmd! {
      cd protocols;
      ./hack/update-generated-proto.sh;
    };
    result.unwrap();
}

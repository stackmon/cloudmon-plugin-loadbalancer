Cloudmon-plugin-loadbalancer
============================

CloudMon plugin to monitor LoadBalancer service: response latency and errors of
servers behind the loadbalancer

.. code-block:: yaml

   timeout: 2
   interval: 10
   loadbalancers:
    - address: lb.test.com
      name: test-lb
      listeners:
        - port: 80
          mode: http
        - port: 443
          mode: https
        - port: 3333
          mode: tcp
   statsd:
     server: 192.168.1.1:8125
     prefix: cloudmon.env.zone
